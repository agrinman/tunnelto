pub mod console_log;
pub use self::console_log::*;
use super::*;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use hyper::Uri;
use std::net::SocketAddr;
use std::vec;
use uuid::Uuid;
use warp::Filter;

#[derive(Debug, Clone)]
pub struct Request {
    id: String,
    status: u16,
    is_replay: bool,
    path: Option<String>,
    method: Option<String>,
    headers: Vec<(String, String)>,
    body_data: Vec<u8>,
    response_headers: Vec<(String, String)>,
    response_data: Vec<u8>,
    started: chrono::NaiveDateTime,
    completed: chrono::NaiveDateTime,
    entire_request: Vec<u8>,
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

pub fn start_introspect_web_dashboard(config: Config) -> SocketAddr {
    let dash_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], config.dashboard_port));

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
            .and_then(move |id| replay_request(id, config.clone())))
        .or(css)
        .or(logo);

    let (web_explorer_address, explorer_server) =
        warp::serve(web_explorer).bind_ephemeral(dash_addr);
    tokio::spawn(explorer_server);

    web_explorer_address
}

#[derive(Debug, Clone)]
pub struct IntrospectChannels {
    pub request: UnboundedSender<Vec<u8>>,
    pub response: UnboundedSender<Vec<u8>>,
}

pub fn introspect_stream() -> IntrospectChannels {
    let id = Uuid::new_v4();
    let (request_tx, request_rx) = unbounded::<Vec<u8>>();
    let (response_tx, response_rx) = unbounded::<Vec<u8>>();

    tokio::spawn(async move { collect_stream(id, request_rx, response_rx).await });

    IntrospectChannels {
        request: request_tx,
        response: response_tx,
    }
}

async fn collect_stream(
    id: Uuid,
    mut request_rx: UnboundedReceiver<Vec<u8>>,
    mut response_rx: UnboundedReceiver<Vec<u8>>,
) {
    let started = chrono::Local::now().naive_local();
    let mut collected_request: Vec<u8> = vec![];
    let mut collected_response: Vec<u8> = vec![];

    while let Some(next) = request_rx.next().await {
        collected_request.extend(next);
    }

    while let Some(next) = response_rx.next().await {
        collected_response.extend(next);
    }

    // collect the request
    let mut request_headers = [httparse::EMPTY_HEADER; 100];
    let mut request = httparse::Request::new(&mut request_headers);

    let parts_len = match request.parse(collected_request.as_slice()) {
        Ok(httparse::Status::Complete(len)) => len,
        _ => {
            warn!("incomplete request received");
            return;
        }
    };
    let body_data = collected_request.as_slice()[parts_len..].to_vec();

    // collect the response
    let mut response_headers = [httparse::EMPTY_HEADER; 100];
    let mut response = httparse::Response::new(&mut response_headers);

    let parts_len = match response.parse(&collected_response.as_slice()) {
        Ok(httparse::Status::Complete(len)) => len,
        _ => 0,
    };
    let response_data = collected_response.as_slice()[parts_len..].to_vec();

    console_log::log(&request, &response);

    let stored_request = Request {
        id: id.to_string(),
        path: request.path.map(String::from),
        method: request.method.map(String::from),
        headers: request_headers
            .iter()
            .filter(|h| *h != &httparse::EMPTY_HEADER)
            .map(|h| {
                (
                    h.name.to_string(),
                    std::str::from_utf8(h.value).unwrap_or("???").to_string(),
                )
            })
            .collect(),
        body_data,
        status: response.code.unwrap_or(0),
        response_headers: response_headers
            .iter()
            .filter(|h| *h != &httparse::EMPTY_HEADER)
            .map(|h| {
                (
                    h.name.to_string(),
                    std::str::from_utf8(h.value).unwrap_or("???").to_string(),
                )
            })
            .collect(),
        response_data,
        started,
        completed: chrono::Local::now().naive_local(),
        is_replay: false,
        entire_request: collected_request,
    };

    REQUESTS
        .write()
        .unwrap()
        .insert(stored_request.id.clone(), stored_request);
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
        Ok(v) => {
            body.data_type = DataType::Json;
            body.content = serde_json::to_string(&v).ok();
        }
        _ => {}
    }

    body
}

async fn replay_request(
    rid: String,
    config: Config,
) -> Result<Box<dyn warp::Reply>, warp::reject::Rejection> {
    let request: Request = match REQUESTS.read().unwrap().get(&rid) {
        Some(r) => r.clone(),
        None => return Err(warp::reject::not_found()),
    };

    let (tx, rx) = unbounded::<ControlPacket>();
    tokio::spawn(async move {
        // keep the rx alive
        let mut rx = rx;
        while let Some(_) = rx.next().await {
            // do nothing
        }
    });

    let tx = local::setup_new_stream(config, tx, StreamId::generate()).await;

    // send the data to the stream
    if let Some(mut tx) = tx {
        let _ = tx.send(StreamMessage::Data(request.entire_request)).await;
    } else {
        error!("failed to replay request: local tunnel could not connect");
        return Err(warp::reject::not_found());
    }

    Ok(Box::new(warp::redirect(Uri::from_static("/"))))
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
