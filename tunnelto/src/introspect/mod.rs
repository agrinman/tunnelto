pub mod console_log;
pub use self::console_log::*;
use super::*;
use bytes::Buf;
use futures::{Stream, StreamExt};
use hyper::body::HttpBody;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use uuid::Uuid;
use warp::http::HeaderMap;
use warp::http::Method;
use warp::path::FullPath;
use warp::Filter;

type HttpClient = hyper::Client<HttpsConnector<HttpConnector>>;

#[derive(Debug, Clone)]
pub struct Request {
    id: String,
    status: u16,
    is_replay: bool,
    path: String,
    query: Option<String>,
    method: Method,
    headers: HashMap<String, Vec<String>>,
    body_data: Vec<u8>,
    response_headers: HashMap<String, Vec<String>>,
    response_data: Vec<u8>,
    started: chrono::NaiveDateTime,
    completed: chrono::NaiveDateTime,
}

impl Request {
    pub fn path_and_query(&self) -> String {
        if let Some(query) = self.query.as_ref() {
            format!("{}?{}", self.path, query)
        } else {
            self.path.clone()
        }
    }
}

impl Request {
    pub fn elapsed(&self) -> String {
        let duration = self.completed - self.started;
        if duration.num_seconds() == 0 {
            format!("{}ms", duration.num_milliseconds())
        } else {
            format!("{}s", duration.num_seconds())
        }
    }
}

lazy_static::lazy_static! {
    pub static ref REQUESTS:Arc<RwLock<HashMap<String, Request>>> = Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Clone)]
pub struct IntrospectionAddrs {
    pub forward_address: SocketAddr,
    pub web_explorer_address: SocketAddr,
}

#[derive(Debug)]
pub enum ForwardError {
    IncomingRead,
    InvalidURL,
    InvalidRequest,
    LocalServerError,
}
impl warp::reject::Reject for ForwardError {}

pub fn start_introspection_server(config: Config) -> IntrospectionAddrs {
    let port = if config.scheme.as_str() == "http" {
        let port = config
            .local_port
            .as_ref()
            .map(|p| p.as_str())
            .unwrap_or("8000");
        format!(":{}", port)
    } else {
        config
            .local_port
            .as_ref()
            .map(|p| format!(":{}", p))
            .unwrap_or(String::new())
    };

    let local_addr = format!("{}://{}{}", &config.scheme, &config.local_host, port);

    let https = hyper_tls::HttpsConnector::new();
    let http_client = hyper::Client::builder().build::<_, hyper::Body>(https);

    let get_client = move || {
        let client = http_client.clone();
        warp::any().map(move || client.clone()).boxed()
    };

    let intercept = warp::any()
        .and(warp::any().map(move || local_addr.clone()))
        .and(warp::method())
        .and(warp::path::full())
        .and(opt_raw_query())
        .and(warp::header::headers_cloned())
        .and(warp::body::stream())
        .and(get_client())
        .and_then(forward);

    let (forward_address, intercept_server) =
        warp::serve(intercept).bind_ephemeral(SocketAddr::from(([0, 0, 0, 0], 0)));
    tokio::spawn(intercept_server);

    let css = warp::get().and(warp::path!("static" / "css" / "styles.css").map(|| {
        let mut res = warp::http::Response::new(warp::hyper::Body::from(include_str!(
            "../../static/css/styles.css"
        )));
        res.headers_mut().insert(
            warp::http::header::CONTENT_TYPE,
            warp::http::header::HeaderValue::from_static("text/css"),
        );
        res
    }));
    let logo = warp::get().and(warp::path!("static" / "img" / "logo.png").map(|| {
        let mut res = warp::http::Response::new(warp::hyper::Body::from(
            include_bytes!("../../static/img/logo.png").to_vec(),
        ));
        res.headers_mut().insert(
            warp::http::header::CONTENT_TYPE,
            warp::http::header::HeaderValue::from_static("image/png"),
        );
        res
    }));
    let forward_clone = forward_address.clone();

    let web_explorer = warp::get()
        .and(warp::path::end())
        .and_then(inspector)
        .or(warp::get()
            .and(warp::path("detail"))
            .and(warp::path::param())
            .and_then(request_detail))
        .or(warp::post()
            .and(warp::path("replay"))
            .and(warp::path::param())
            .and(get_client())
            .and_then(move |id, client| replay_request(id, client, forward_clone.clone())))
        .or(css)
        .or(logo);

    let (web_explorer_address, explorer_server) =
        if let Some(dashboard_address) = config.dashboard_address {
            warp::serve(web_explorer).bind_ephemeral(
                SocketAddr::from_str(dashboard_address.as_str())
                    .expect("Failed to bind to supplied local dashboard address"),
            )
        } else {
            warp::serve(web_explorer).bind_ephemeral(SocketAddr::from(([0, 0, 0, 0], 0)))
        };

    tokio::spawn(explorer_server);

    IntrospectionAddrs {
        forward_address,
        web_explorer_address,
    }
}

async fn forward(
    local_addr: String,
    method: Method,
    path: FullPath,
    query: Option<String>,
    headers: HeaderMap,
    mut body: impl Stream<Item = Result<impl Buf, warp::Error>> + Send + Sync + Unpin + 'static,
    client: HttpClient,
) -> Result<Box<dyn warp::Reply>, warp::reject::Rejection> {
    let started = chrono::Utc::now().naive_utc();

    let mut request_headers = HashMap::new();
    headers.keys().for_each(|k| {
        let values = headers
            .get_all(k)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|s| s.to_owned())
            .collect();
        request_headers.insert(k.as_str().to_owned(), values);
    });

    let mut collected: Vec<u8> = vec![];

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| {
            log::error!("error reading incoming buffer: {:?}", e);
            warp::reject::custom(ForwardError::IncomingRead)
        })?;

        collected.extend_from_slice(chunk.chunk())
    }

    let query_str = if let Some(query) = query.as_ref() {
        format!("?{}", query)
    } else {
        String::new()
    };

    let url = format!("{}{}{}", local_addr, path.as_str(), query_str);
    log::debug!("forwarding to: {}", &url);

    let mut request = hyper::Request::builder()
        .method(method.clone())
        .version(hyper::Version::HTTP_11)
        .uri(url.parse::<hyper::Uri>().map_err(|e| {
            log::error!("invalid incoming url: {}, error: {:?}", url, e);
            warp::reject::custom(ForwardError::InvalidURL)
        })?);

    for header in headers {
        if let Some(header_name) = header.0 {
            request = request.header(header_name, header.1)
        }
    }

    // let _ = request.headers_mut().replace(&mut headers);
    let request = request
        .body(hyper::Body::from(collected.clone()))
        .map_err(|e| {
            log::error!("failed to build request: {:?}", e);
            warp::reject::custom(ForwardError::InvalidRequest)
        })?;

    let response = client.request(request).await.map_err(|e| {
        log::error!("local server error: {:?}", e);
        warp::reject::custom(ForwardError::LocalServerError)
    })?;

    let mut response_headers = HashMap::new();
    response.headers().keys().for_each(|k| {
        let values = response
            .headers()
            .get_all(k)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|s| s.to_owned())
            .collect();
        response_headers.insert(k.as_str().to_owned(), values);
    });

    let (parts, mut body) = response.into_parts();

    let mut response_data = vec![];
    while let Some(next) = body.data().await {
        let chunk = next.map_err(|e| {
            log::error!("error reading local response: {:?}", e);
            warp::reject::custom(ForwardError::LocalServerError)
        })?;

        response_data.extend_from_slice(&chunk);
    }

    let stored_request = Request {
        id: Uuid::new_v4().to_string(),
        status: parts.status.as_u16(),
        path: path.as_str().to_owned(),
        query,
        method,
        headers: request_headers,
        body_data: collected,
        response_headers,
        response_data: response_data.clone(),
        started,
        completed: chrono::Utc::now().naive_utc(),
        is_replay: false,
    };

    REQUESTS
        .write()
        .unwrap()
        .insert(stored_request.id.clone(), stored_request);

    Ok(Box::new(warp::http::Response::from_parts(
        parts,
        response_data,
    )))
}

#[derive(Debug, Clone, askama::Template)]
#[template(path = "index.html")]
struct Inspector {
    requests: Vec<Request>,
}

#[derive(Debug, Clone, askama::Template)]
#[template(path = "detail.html")]
struct InspectorDetail {
    request: Request,
    incoming: BodyData,
    response: BodyData,
}

#[derive(Debug, Clone)]
struct BodyData {
    data_type: DataType,
    content: Option<String>,
    raw: String,
}

impl AsRef<BodyData> for BodyData {
    fn as_ref(&self) -> &BodyData {
        &self
    }
}

#[derive(Debug, Clone)]
enum DataType {
    Json,
    Unknown,
}

async fn inspector() -> Result<Page<Inspector>, warp::reject::Rejection> {
    let mut requests: Vec<Request> = REQUESTS
        .read()
        .unwrap()
        .values()
        .map(|r| r.clone())
        .collect();
    requests.sort_by(|a, b| b.completed.cmp(&a.completed));
    let inspect = Inspector { requests };
    Ok(Page(inspect))
}

async fn request_detail(rid: String) -> Result<Page<InspectorDetail>, warp::reject::Rejection> {
    let request: Request = match REQUESTS.read().unwrap().get(&rid) {
        Some(r) => r.clone(),
        None => return Err(warp::reject::not_found()),
    };

    let detail = InspectorDetail {
        incoming: get_body_data(&request.body_data),
        response: get_body_data(&request.response_data),
        request,
    };

    Ok(Page(detail))
}

fn get_body_data(input: &[u8]) -> BodyData {
    let mut body = BodyData {
        data_type: DataType::Unknown,
        content: None,
        raw: std::str::from_utf8(input)
            .map(|s| s.to_string())
            .unwrap_or("No UTF-8 Data".to_string()),
    };

    match serde_json::from_slice::<serde_json::Value>(input) {
        Ok(serde_json::Value::Object(map)) => {
            body.data_type = DataType::Json;
            body.content = serde_json::to_string_pretty(&map).ok();
        }
        Ok(serde_json::Value::Array(arr)) => {
            body.data_type = DataType::Json;
            body.content = serde_json::to_string_pretty(&arr).ok();
        }
        _ => {}
    }

    body
}

async fn replay_request(
    rid: String,
    client: HttpClient,
    addr: SocketAddr,
) -> Result<Box<dyn warp::Reply>, warp::reject::Rejection> {
    let request: Request = match REQUESTS.read().unwrap().get(&rid) {
        Some(r) => r.clone(),
        None => return Err(warp::reject::not_found()),
    };

    let query_str = if let Some(query) = request.query.as_ref() {
        format!("?{}", query)
    } else {
        String::new()
    };

    let url = format!(
        "http://localhost:{}{}{}",
        addr.port(),
        &request.path,
        query_str
    );

    let mut new_request = hyper::Request::builder()
        .method(request.method)
        .version(hyper::Version::HTTP_11)
        .uri(url.parse::<hyper::Uri>().map_err(|e| {
            log::error!("invalid incoming url: {}, error: {:?}", url, e);
            warp::reject::custom(ForwardError::InvalidURL)
        })?);

    for (header, values) in &request.headers {
        for v in values {
            new_request = new_request.header(header, v)
        }
    }

    let new_request = new_request
        .body(hyper::Body::from(request.body_data))
        .map_err(|e| {
            log::error!("failed to build request: {:?}", e);
            warp::reject::custom(ForwardError::InvalidRequest)
        })?;

    let _ = client.request(new_request).await.map_err(|e| {
        log::error!("local server error: {:?}", e);
        warp::reject::custom(ForwardError::LocalServerError)
    })?;

    let response = warp::http::Response::builder()
        .status(warp::http::StatusCode::SEE_OTHER)
        .header(warp::http::header::LOCATION, "/")
        .body(b"".to_vec());

    Ok(Box::new(response))
}

struct Page<T>(T);

impl<T> warp::reply::Reply for Page<T>
where
    T: askama::Template + Send + 'static,
{
    fn into_response(self) -> warp::reply::Response {
        let res = self.0.render().unwrap();

        warp::http::Response::builder()
            .status(warp::http::StatusCode::OK)
            .header(warp::http::header::CONTENT_TYPE, "text/html")
            .body(res.into())
            .unwrap()
    }
}

fn opt_raw_query() -> impl Filter<Extract = (Option<String>,), Error = Infallible> + Copy {
    warp::filters::query::raw()
        .map(|q| Some(q))
        .or(warp::any().map(|| None))
        .unify()
}
