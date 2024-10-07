mod hyper;
mod rest;

use std::collections::HashMap;
use ::hyper::{body, HeaderMap, header::HeaderValue};
use bytes::Bytes;
use tokio::time::{sleep, Duration};
use rust_decimal::Decimal;
use crate::hyper::{compile_uri, create_hyper, create_request_builder, HyperClient};
use serde::Deserialize;
use crate::rest::{MethodResponse, ResponseTransformerError, RestApiMethod, RestApiMethodBuilder, RestApiMethodParam, ValidateResponseError};
use rust_decimal::prelude::ToPrimitive;
use serde::de::DeserializeOwned;
use log::{info, warn};

const BASE_URL: &str = "https://api.coingecko.com";
const SLEEP_BETWEEN_REQUESTS_INITIAL_MS: u64 = 10000;
const SLEEP_BETWEEN_REQUESTS_STEP_MS: u64 = 500;
const RATES_TOKENS_PER_REQUEST: usize = 500;
const COOLDOWN_SEC: f32 = 60_f32;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{0}")]
    Runtime(String),
}

pub type CoingeckoTokenPlatforms = HashMap<String, Option<String>>;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct CgCoin {
    id: String,
    symbol: String,
    name: String,
    platforms: CoingeckoTokenPlatforms,
    rate: Option<Decimal>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct CgRate {
    #[serde(default, with = "rust_decimal::serde::arbitrary_precision_option")]
    usd: Option<Decimal>,
    // #[serde(default, with = "rust_decimal::serde::arbitrary_precision_option")]
    // usd_market_cap: Option<Decimal>, //@TODO Scale exceeds the maximum precision allowed: 35 > 28
    last_updated_at: Option<u32>, // Important! Can be 0
}

pub type CgRates = HashMap<String, CgRate>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    env_logger::init();
    info!("Sync coingecko tokens.");
    let hyper = create_hyper();
    let all_coins_rest_api_method_template = build_all_coins_rest_api_method_builder_template();
    let coin_rates_rest_api_method_template =
        build_coin_rates_rest_api_method_builder_template();

    let mut sleep_between_requests_ms = SLEEP_BETWEEN_REQUESTS_INITIAL_MS;
    loop {
        let mut coins = match fetch_coins(
            &hyper,
            &all_coins_rest_api_method_template,
            &mut sleep_between_requests_ms,
        )
            .await
        {
            Ok(x) => x,
            Err(e) => {
                return Err(get_execution_error(Error::Runtime(format!(
                    "fetch_coins error: {}",
                    e
                ))));
            }
        };
        let coins_len = coins.len();
        info!("Fetched {} tokens.", coins_len);

        let iterations = coins_len.div_ceil(RATES_TOKENS_PER_REQUEST);

        let mut rates_found_num = 0;
        for i in 0..iterations {
            let start = i * RATES_TOKENS_PER_REQUEST;
            let end = if i == iterations - 1 {
                coins_len
            } else {
                start + RATES_TOKENS_PER_REQUEST
            };
            // println!("i{} s{} e{}", i, start, end);
            let ids: Vec<String> = coins[start..end].iter().map(|e| e.id.clone()).collect();
            // println!("{:?}", ids);

            info!(
                    "Sleeping {}ms between requests...",
                    sleep_between_requests_ms
                );
            sleep(Duration::from_millis(sleep_between_requests_ms)).await;
            let rates = match fetch_rates(
                &hyper,
                &coin_rates_rest_api_method_template,
                ids,
                &mut sleep_between_requests_ms,
            )
                .await
            {
                Ok(x) => x,
                Err(e) => {
                    return Err(get_execution_error(Error::Runtime(format!(
                        "fetch_rates error: {}",
                        e
                    ))));
                }
            };
            rates_found_num += rates.len();
            info!(
                    "Fetched {} ({}) rates of {} tokens.",
                    rates.len(),
                    rates_found_num,
                    coins_len
                );
            for (id, rate) in &rates {
                if rate.usd.is_some() {
                    let rate_usd = if rate.usd.unwrap().is_zero() {
                        None
                    } else {
                        rate.usd
                    };
                    if let Some(coin) = coins.iter_mut().find(|x| { x.id == id.as_str() }) {
                        coin.rate = rate_usd;
                    }
                }
            }
        }


        println!("Example:");
        println!("{:?}", coins[0]);
        println!("{:?}", coins[10]);
        println!("{:?}", coins[100]);
        println!("{:?}", coins[1000]);
        println!("{:?}", coins[10000]);

        info!("Done, sleeping {} seconds to repeat...", COOLDOWN_SEC);
        sleep(Duration::from_secs_f32(COOLDOWN_SEC)).await;
    }
}

fn build_all_coins_rest_api_method_builder_template() -> RestApiMethodBuilder {
    let transform_response = |code: &u16,
                              body: &Bytes,
                              headers: &HeaderMap<HeaderValue>|
                              -> Result<MethodResponse, ResponseTransformerError> {
        match validate_response::<Vec<CgCoin>>(code, body, headers) {
            Ok(x) => Ok(MethodResponse::CgAllCoins(x)),
            Err(e) => return Err(ResponseTransformerError::ValidateResponseError(e)),
        }
    };
    let mut builder = RestApiMethod::builder();
    builder
        .set_base_url(BASE_URL.to_owned())
        .set_url("/api/v3/coins/list")
        .add_param(RestApiMethodParam::prevalue(
            "include_platform",
            "true".to_string(),
        ))
        .set_transform_response(transform_response);
    builder
}

fn build_coin_rates_rest_api_method_builder_template() -> RestApiMethodBuilder {
    let transform_response = |code: &u16,
                              body: &Bytes,
                              headers: &HeaderMap<HeaderValue>|
                              -> Result<MethodResponse, ResponseTransformerError> {
        match validate_response::<CgRates>(code, body, headers) {
            Ok(x) => Ok(MethodResponse::CgRates(x)),
            Err(e) => return Err(ResponseTransformerError::ValidateResponseError(e)),
        }
    };
    let mut builder = RestApiMethod::builder();
    builder
        .set_base_url(BASE_URL.to_owned())
        .set_url("/api/v3/simple/price")
        .add_param(RestApiMethodParam::prevalue(
            "vs_currencies",
            "usd".to_string(),
        ))
        .add_param(RestApiMethodParam::prevalue("precision", "18".to_string()))
        .add_param(RestApiMethodParam::prevalue(
            "include_last_updated_at",
            "true".to_string(),
        ))
        .add_param(RestApiMethodParam::prevalue(
            "include_market_cap",
            "true".to_string(),
        ))
        .set_transform_response(transform_response);
    builder
}

async fn fetch_coins(
    hyper: &HyperClient,
    all_coins_rest_api_method_template: &RestApiMethodBuilder,
    sleep_between_requests_ms: &mut u64,
) -> Result<Vec<CgCoin>, String> {
    let all_coins_rest_api_method = all_coins_rest_api_method_template.clone().build();
    let all_coins_response =
        match request(&hyper, all_coins_rest_api_method, sleep_between_requests_ms).await {
            Ok(x) => x,
            Err(e) => {
                return Err(format!("All coins request error: {}", e));
            }
        };
    if let MethodResponse::CgAllCoins(coins) = all_coins_response {
        Ok(coins)
    } else {
        Err("Api method response doesn't content correct variant.".to_string())
    }
}

async fn fetch_rates(
    hyper: &HyperClient,
    coin_rates_rest_api_method_template: &RestApiMethodBuilder,
    ids: Vec<String>,
    sleep_between_requests_ms: &mut u64,
) -> Result<CgRates, String> {
    let mut coin_rates_rest_api_method_builder = coin_rates_rest_api_method_template.clone();
    coin_rates_rest_api_method_builder
        .add_param(RestApiMethodParam::prevalue("ids", ids.join(",")));
    let coin_rates_response = match request(
        hyper,
        coin_rates_rest_api_method_builder.build(),
        sleep_between_requests_ms,
    )
        .await
    {
        Ok(x) => x,
        Err(e) => return Err(format!("All coins request error: {}", e)),
    };
    if let MethodResponse::CgRates(rates) = coin_rates_response {
        // println!("Rates num: {}", rates.len());
        // println!("{:?}", rates);
        Ok(rates)
    } else {
        Err("Api method response doesn't content correct variant.".to_string())
    }
}

async fn request(
    hyper: &HyperClient,
    rest_api_method: RestApiMethod,
    sleep_between_requests_ms: &mut u64,
) -> Result<MethodResponse, String> {
    #[allow(while_true)]
    while true {
        let uri = compile_uri(&rest_api_method)?;
        // println!("{}", uri);
        let request_builder = create_request_builder().uri(uri);
        let request = request_builder.body(String::new()).unwrap();
        let res = hyper.request(request).await.unwrap();
        let status_code = res.status().as_u16();
        //@TODO should try aggregate
        // https://docs.rs/serde_json/latest/serde_json/fn.from_reader.html
        let headers = res.headers().clone();
        // let body = hyper::body::to_bytes(res.body().);
        let body = body::to_bytes(res.into_body()).await.unwrap();
        match (rest_api_method.transform_response)(&status_code, &body, &headers) {
            Ok(x) => return Ok(x),
            Err(e) => match &e {
                ResponseTransformerError::ValidateResponseError(t_e) => match t_e {
                    ValidateResponseError::Banned(banned_for) => {
                        if let Some(seconds) = banned_for {
                            *sleep_between_requests_ms += SLEEP_BETWEEN_REQUESTS_STEP_MS;
                            let seconds = seconds + 1;
                            warn!("Banned for {} seconds, sleeping...", seconds);
                            sleep(Duration::from_secs_f32(seconds.to_f32().unwrap())).await;
                            continue;
                        } else {
                            return Err(format!("Banned for unknown time: {}", e));
                        }
                    }
                    _ => return Err(format!("Unhandled Validate Response Error: {}", e)),
                },
                #[allow(unreachable_patterns)] // хз почему он это видит анричбл
                _ => return Err(format!("Unhandled Transform Response Error: {}", e).to_string()),
            },
        }
    }
    Err("Unexpected end of loop.".to_string())
}

pub fn validate_response<M: DeserializeOwned>(
    code: &u16,
    body: &Bytes,
    headers: &HeaderMap<HeaderValue>,
) -> Result<M, ValidateResponseError> {
    // println!("Code: {}", code);
    // println!("Response: {}", String::from_utf8(body.to_vec()).unwrap());
    match *code {
        200 => {
            let response: M = match serde_json::from_slice(body) {
                Ok(x) => x,
                Err(e) => {
                    info!("{}", String::from_utf8(body.to_vec()).unwrap());
                    return Err(ValidateResponseError::FailToParse(e.to_string()));
                }
            };
            Ok(response)
        }
        429 => {
            if let Some(retry_after) = headers.get("retry-after") {
                let period_seconds: u32 = retry_after.to_str().unwrap().parse().unwrap();
                Err(ValidateResponseError::Banned(Some(period_seconds)))
            } else {
                Err(ValidateResponseError::Banned(None))
            }
        }
        _ => Err(ValidateResponseError::UnexpectedResponseCode(*code)),
    }
}

fn get_execution_error(error: Error) -> Box<dyn std::error::Error + 'static> {
    Box::new(error)
}