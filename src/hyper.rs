use hyper::{client::HttpConnector, Client, Request};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use crate::rest::{RequestMethod, RestApiMethod};
use url::Url;
use hyper::http::request::Builder;

pub type HyperClient = Client<HttpsConnector<HttpConnector>, String>;

pub fn create_hyper() -> HyperClient {
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http2()
        .build();
    let hyper = Client::builder()
        .http2_only(true)
        .build::<_, String>(https);
    hyper
}

pub fn compile_uri(rest_api_method: &RestApiMethod) -> Result<String, String> {
    let mut url = rest_api_method.url.to_string();
    for param in &rest_api_method.route_params {
        let val = match &param.value {
            Some(x) => x,
            None => {
                return Err(format!(
                    "compile_uri Required route param {} not set.",
                    param.key
                ));
            }
        };
        let key = format!("{{{}}}", param.key);
        url = url.replace(&key, val);
    }
    let mut res = Url::parse(format!("{}{}", rest_api_method.base_url, url).as_str()).unwrap();
    // @TODO remove it.
    // Переделать в методах params в query_params
    if rest_api_method.method == RequestMethod::Get {
        for param in &rest_api_method.params.items {
            if let Some(value) = &param.value {
                res.query_pairs_mut().append_pair(param.key, value.as_str());
            } else if param.is_required {
                return Err(format!("compile_uri Required param {} not set.", param.key));
            }
        }
    }
    for param in &rest_api_method.query_params.items {
        if let Some(value) = &param.value {
            res.query_pairs_mut().append_pair(param.key, value.as_str());
        } else if param.is_required {
            return Err(format!("compile_uri Required param {} not set.", param.key));
        }
    }
    Ok(res.to_string())
}

pub fn create_request_builder() -> Builder {
    Request::builder().header(
        "User-Agent",
        format!("cg_req/{}", env!("CARGO_PKG_VERSION")),
    )
}
