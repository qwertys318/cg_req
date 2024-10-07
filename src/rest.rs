use serde::ser::{Error, SerializeStruct};
use serde::{Serialize, Serializer};
use bytes::Bytes;
use hyper::HeaderMap;
use hyper::header::HeaderValue;
use crate::{CgCoin, CgRates};

#[derive(Clone, PartialEq, strum_macros::EnumString, strum_macros::Display)]
pub enum RequestMethod {
    Get,
    Post,
}

#[derive(Clone)]
pub struct RestApiMethodParam {
    pub key: &'static str,
    pub value: Option<String>,
    pub is_required: bool,
}

#[allow(dead_code)]
impl RestApiMethodParam {
    pub fn prevalue(key: &'static str, value: String) -> Self {
        Self {
            key,
            value: Some(value),
            is_required: true,
        }
    }
    pub fn required(key: &'static str) -> Self {
        Self {
            key,
            value: None,
            is_required: true,
        }
    }
    pub fn optional(key: &'static str) -> Self {
        Self {
            key,
            value: None,
            is_required: false,
        }
    }
}

#[derive(Clone)]
pub struct RestApiMethodRouteParam {
    pub key: &'static str,
    pub value: Option<String>,
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ResponseTransformerError {
    #[error("ResponseTransformerError::ValidateResponseError {0}")]
    ValidateResponseError(ValidateResponseError),
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ValidateResponseError {
    #[error("ValidateResponseError::FailToParse {0}")]
    FailToParse(String),
    #[error("ValidateResponseError::Banned")]
    Banned(Option<u32>),
    #[error("ValidateResponseError::InnerError {0}")]
    InnerError(String),
    #[error("ValidateResponseError::KeyExpired")]
    KeyExpired,
    #[error("ValidateResponseError::UnexpectedResponseCode {0}")]
    UnexpectedResponseCode(u16),
}

#[allow(dead_code)]
#[non_exhaustive]
pub enum RequestConfiguratorParams {
    NextKey(String),
}

#[derive(Clone)]
pub struct RestApiMethodParamBunch {
    pub items: Vec<RestApiMethodParam>,
}

impl Serialize for RestApiMethodParamBunch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
    {
        let mut filtered: Vec<&RestApiMethodParam> = vec![];
        for param in &self.items {
            if param.value.is_some() {
                filtered.push(param);
            } else if param.is_required {
                return Err(Error::custom(format!(
                    "Required param {} is not set.",
                    param.key
                )));
            }
        }
        let mut state = serializer.serialize_struct("RestApiMethodParamBunch", filtered.len())?;
        for param in filtered {
            state.serialize_field(param.key, param.value.as_ref().unwrap())?;
        }
        state.end()
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct RestApiMethod {
    pub base_url: String,
    pub method: RequestMethod,
    pub url: &'static str,
    pub params: RestApiMethodParamBunch,
    pub query_params: RestApiMethodParamBunch,
    pub route_params: Vec<RestApiMethodRouteParam>,
    pub transform_response: ResponseTransformer,
    pub configure_request: Option<RequestConfigurator>,
}

pub type RequestConfigurator = fn(ram: &mut RestApiMethod, params: RequestConfiguratorParams);

pub type ResponseTransformer = fn(
    code: &u16,
    body: &Bytes,
    headers: &HeaderMap<HeaderValue>,
) -> Result<MethodResponse, ResponseTransformerError>;

pub enum MethodResponse {
    // CoinGecko
    CgAllCoins(Vec<CgCoin>),
    CgRates(CgRates),
}

#[derive(Clone)]
pub struct RestApiMethodBuilder {
    base_url: Option<String>,
    method: RequestMethod,
    url: Option<&'static str>,
    params: Vec<RestApiMethodParam>,
    query_params: Vec<RestApiMethodParam>,
    route_params: Vec<RestApiMethodRouteParam>,
    transform_response: Option<ResponseTransformer>,
    configure_request: Option<RequestConfigurator>,
}

#[allow(dead_code)]
impl RestApiMethodBuilder {
    pub fn new() -> Self {
        RestApiMethodBuilder {
            base_url: None,
            method: RequestMethod::Get,
            url: None,
            params: vec![],
            query_params: vec![],
            route_params: vec![],
            transform_response: None,
            configure_request: None,
        }
    }
    pub fn set_base_url(&mut self, base_url: String) -> &mut Self {
        self.base_url = Some(base_url);
        self
    }
    pub fn set_method(&mut self, method: RequestMethod) -> &mut Self {
        self.method = method;
        self
    }
    pub fn set_url(&mut self, url: &'static str) -> &mut Self {
        self.url = Some(url);
        self
    }
    pub fn add_route_param(&mut self, param: RestApiMethodRouteParam) -> &mut Self {
        self.route_params.push(param);
        self
    }
    pub fn add_param(&mut self, param: RestApiMethodParam) -> &mut Self {
        self.params.push(param);
        self
    }
    pub fn add_query_param(&mut self, param: RestApiMethodParam) -> &mut Self {
        self.query_params.push(param);
        self
    }
    pub fn set_transform_response(&mut self, transform_response: ResponseTransformer) -> &mut Self {
        self.transform_response = Some(transform_response);
        self
    }
    pub fn set_configure_request(&mut self, configure_request: RequestConfigurator) -> &mut Self {
        self.configure_request = Some(configure_request);
        self
    }
    pub fn build(&self) -> RestApiMethod {
        RestApiMethod {
            base_url: self
                .base_url
                .clone()
                .expect("RestApiMethodBuilder base_url was not set"),
            method: self.method.clone(),
            url: self.url.expect("RestApiMethodBuilder url was not set"),
            params: RestApiMethodParamBunch {
                items: self.params.clone(),
            },
            query_params: RestApiMethodParamBunch {
                items: self.query_params.clone(),
            },
            route_params: self.route_params.clone(),
            transform_response: self
                .transform_response
                .expect("RestApiMethodBuilder transform_response was not set"),
            configure_request: self.configure_request,
        }
    }
}

impl RestApiMethod {
    pub fn builder() -> RestApiMethodBuilder {
        RestApiMethodBuilder::new()
    }
    pub fn set_param_value(&mut self, key: &'static str, value: String) -> Result<(), String> {
        if let Some(param) = self.params.items.iter_mut().find(|e| e.key == key) {
            if param.value.is_some() {
                return Err(format!("Param '{}' already set.", key));
            }
            param.value = Some(value);
            Ok(())
        } else {
            Err(format!("Param '{}' was not found.", key))
        }
    }
    pub fn set_query_param_value(
        &mut self,
        key: &'static str,
        value: String,
    ) -> Result<(), String> {
        if let Some(param) = self.query_params.items.iter_mut().find(|e| e.key == key) {
            if param.value.is_some() {
                return Err(format!("Param '{}' already set.", key));
            }
            param.value = Some(value);
            Ok(())
        } else {
            Err(format!("Param '{}' was not found.", key))
        }
    }
    pub fn set_route_param_value(
        &mut self,
        key: &'static str,
        value: String,
    ) -> Result<(), String> {
        if let Some(param) = self.route_params.iter_mut().find(|e| e.key == key) {
            if param.value.is_some() {
                return Err(format!("Route param '{}' already set.", key));
            }
            param.value = Some(value);
            Ok(())
        } else {
            Err(format!("Route param '{}' was not found.", key))
        }
    }
    pub fn convert_params_into_json_string(&self) -> Result<String, String> {
        Ok(serde_json::to_string(&self.params).unwrap())
    }
}