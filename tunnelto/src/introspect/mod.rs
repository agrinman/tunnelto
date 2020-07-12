pub mod console_log;
pub use self::console_log::*;
use super::*;
use std::net::{SocketAddr};
use warp::Filter;
use warp::http::Method;
use warp::path::FullPath;
use warp::http::HeaderMap;
use futures::{Stream, StreamExt};
use bytes::Buf;
use uuid::Uuid;
use http_body::Body;

#[derive(Debug, Clone)]
pub struct Request {
    id: String,
    status: u16,
    path: String,
    method: Method,
    headers: HashMap<String, Vec<String>>,
    body_data: Vec<u8>,
    response_headers: HashMap<String, Vec<String>>,
    response_data: Vec<u8>,
    timestamp: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, askama::Template)]
#[template(path="base.html")]
struct Inspector {
    requests: Vec<Request>
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
pub enum ForwardError{
    IncomingRead,
    InvalidURL,
    InvalidRequest,
    LocalServerError,
}
impl warp::reject::Reject for ForwardError {}

pub fn start_introspection_server(config: Config) -> IntrospectionAddrs {
    let local_addr = format!("localhost:{}", &config.local_port);

    let intercept = warp::any()
        .and(warp::any().map(move || local_addr.clone()))
        .and(warp::method())
        .and(warp::path::full())
        .and(warp::header::headers_cloned())
        .and(warp::body::stream())
        .and_then(forward);

    let (forward_address, intercept_server) = warp::serve(intercept).bind_ephemeral(SocketAddr::from(([0,0,0,0], 0)));
    tokio::spawn(intercept_server);

    let css = warp::get().and(warp::path!("static" / "css" / "styles.css")
        .map(|| {
            let mut res = warp::http::Response::new(hyper::Body::from(include_str!("../../static/css/styles.css")));
            res.headers_mut().insert(
                warp::http::header::CONTENT_TYPE,
                warp::http::header::HeaderValue::from_static("text/css"),
            );
            res
        }));
    let logo = warp::get().and(warp::path!("static" / "img" / "logo.png")
        .map(|| {
            let mut res = warp::http::Response::new(hyper::Body::from(include_bytes!("../../static/img/logo.png").to_vec()));
            res.headers_mut().insert(
                warp::http::header::CONTENT_TYPE,
                warp::http::header::HeaderValue::from_static("image/png"),
            );
            res
        }));

    let web_explorer = warp::get().and(warp::path::end()).and_then(inspector).or(css).or(logo);

    let (web_explorer_address, explorer_server) = warp::serve(web_explorer).bind_ephemeral(SocketAddr::from(([0,0,0,0], 0)));
    tokio::spawn(explorer_server);

    IntrospectionAddrs { forward_address, web_explorer_address}
}

async fn forward(local_addr: String,
                 method: Method,
                 path: FullPath,
                 mut headers: HeaderMap,
                 mut body: impl Stream<Item = Result<impl Buf, warp::Error>> + Send + Sync + Unpin + 'static)
                 -> Result<Box<dyn warp::Reply>, warp::reject::Rejection>
{
    let now = chrono::Utc::now().naive_utc();
    let mut request_headers = HashMap::new();
    headers.keys().for_each(|k| {
        let values  = headers.get_all(k).iter().filter_map(|v| v.to_str().ok()).map(|s| s.to_owned()).collect();
        request_headers.insert(k.as_str().to_owned(), values);
    });

    let mut collected:Vec<u8> = vec![];

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| {
            log::error!("error reading incoming buffer: {:?}", e);
            warp::reject::custom(ForwardError::IncomingRead)
        })?.to_bytes();

        collected.extend_from_slice(chunk.as_ref())
    }

    let client = hyper::client::Client::new();
    let url = format!("http://{}{}", local_addr, path.as_str());

    let mut request = hyper::Request::builder()
        .method(method.clone())
        .uri(url.parse::<hyper::Uri>().map_err(|e| {
            log::error!("invalid incoming url: {}, error: {:?}", url, e);
            warp::reject::custom(ForwardError::InvalidURL)
        })?);

    let _ = request.headers_mut().replace(&mut headers);
    let request = request.body(hyper::Body::from(collected.clone())).map_err(|e| {
        log::error!("failed to build request: {:?}", e);
        warp::reject::custom(ForwardError::InvalidRequest)
    })?;

    let response = client.request(request).await.map_err(|e| {
        log::error!("local server error: {:?}", e);
        warp::reject::custom(ForwardError::LocalServerError)
    })?;

    let mut response_headers = HashMap::new();
    response.headers().keys().for_each(|k| {
        let values  = headers.get_all(k).iter().filter_map(|v| v.to_str().ok()).map(|s| s.to_owned()).collect();
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
        method,
        headers: request_headers,
        body_data: collected,
        response_headers,
        response_data: response_data.clone(),
        timestamp: now,
    };

    REQUESTS.write().unwrap().insert(stored_request.id.clone(), stored_request);

    Ok(Box::new(warp::http::Response::from_parts(parts, response_data)))
}

async fn inspector() -> Result<Page<Inspector>, warp::reject::Rejection> {
    let mut requests:Vec<Request> = REQUESTS.read().unwrap().values().map(|r| r.clone()).collect();
    requests.sort_by(|a,b| a.timestamp.cmp(&b.timestamp));
    let inspect = Inspector { requests };
    Ok(Page(inspect))
}


struct Page<T>(T);

impl <T> warp::reply::Reply for Page<T> where T:askama::Template + Send + 'static {
    fn into_response(self) -> warp::reply::Response {
        let res = self.0.render().unwrap();

        warp::http::Response::builder().status(warp::http::StatusCode::OK).header(
            warp::http::header::CONTENT_TYPE,
            "text/html",
        ).body(res.into()).unwrap()
    }
}