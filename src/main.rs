use base64::URL_SAFE;
use config::Config;
use id_contact_jwt::sign_and_encrypt_auth_result;
use id_contact_proto::{
    AuthResult, AuthStatus, SessionActivity, StartAuthRequest, StartAuthResponse,
};
use rocket::{form::FromForm, fairing::AdHoc, get, launch, post, response::Redirect, routes, State};
use rocket_contrib::json::Json;
use std::{error::Error as StdError, fmt::Display};

mod config;

#[derive(Debug)]
enum Error {
    Config(config::Error),
    Decode(base64::DecodeError),
    Json(serde_json::Error),
    Utf(std::str::Utf8Error),
    JWT(id_contact_jwt::Error),
}

impl<'r, 'o: 'r> rocket::response::Responder<'r, 'o> for Error {
    fn respond_to(self, request: &'r rocket::Request<'_>) -> rocket::response::Result<'o> {
        let debug_error = rocket::response::Debug::from(self);
        debug_error.respond_to(request)
    }
}

impl From<config::Error> for Error {
    fn from(e: config::Error) -> Error {
        Error::Config(e)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Error {
        Error::Decode(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::Json(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Error {
        Error::Utf(e)
    }
}

impl From<id_contact_jwt::Error> for Error {
    fn from(e: id_contact_jwt::Error) -> Error {
        Error::JWT(e)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config(e) => e.fmt(f),
            Error::Decode(e) => e.fmt(f),
            Error::Utf(e) => e.fmt(f),
            Error::Json(e) => e.fmt(f),
            Error::JWT(e) => e.fmt(f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Config(e) => Some(e),
            Error::Decode(e) => Some(e),
            Error::Utf(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::JWT(e) => Some(e),
        }
    }
}

#[derive(FromForm, Debug)]
struct SessionUpdateData {
    #[field(name = "type")]
    typeval: SessionActivity,
}

#[post("/session/update?<typedata..>")]
async fn session_update(typedata: SessionUpdateData) {
    println!("Session update received: {:?}", typedata.typeval);
}

#[get("/browser/<attributes>/<continuation>/<attr_url>")]
async fn user_oob(
    config: State<'_, config::Config>,
    attributes: String,
    continuation: String,
    attr_url: String,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Succes,
        attributes: Some(attributes),
        session_url: if config.with_session() {
            Some(format!("{}/session/update", config.server_url()))
        } else {
            None
        },
    };
    let auth_result =
        sign_and_encrypt_auth_result(&auth_result, config.signer(), config.encrypter())?;

    let continuation = base64::decode_config(continuation, URL_SAFE)?;
    let continuation = std::str::from_utf8(&continuation)?;

    let attr_url = base64::decode_config(attr_url, URL_SAFE)?;
    let attr_url = std::str::from_utf8(&attr_url)?;

    let client = reqwest::Client::new();
    let result = client
        .post(attr_url)
        .header("Content-Type", "application/jwt")
        .body(auth_result.clone())
        .send()
        .await;
    if let Err(e) = result {
        // Log only
        println!("Failure reporting results: {}", e);
    } else {
        println!("Reported result jwe {} to {}", &auth_result, attr_url);
    }

    println!("Redirecting user to {}", continuation);
    Ok(Redirect::to(continuation.to_string()))
}

#[get("/browser/<attributes>/<continuation>")]
async fn user_inline(
    config: State<'_, config::Config>,
    attributes: String,
    continuation: String,
) -> Result<Redirect, Error> {
    let attributes = base64::decode_config(attributes, URL_SAFE)?;
    let attributes: Vec<String> = serde_json::from_slice(&attributes)?;
    let attributes = config.map_attributes(&attributes)?;
    let auth_result = AuthResult {
        status: AuthStatus::Succes,
        attributes: Some(attributes),
        session_url: if config.with_session() {
            Some(format!("{}/session/update", config.server_url()))
        } else {
            None
        },
    };
    let auth_result =
        sign_and_encrypt_auth_result(&auth_result, config.signer(), config.encrypter())?;

    let continuation = base64::decode_config(continuation, URL_SAFE)?;
    let continuation = std::str::from_utf8(&continuation)?;

    println!(
        "Redirecting user to {} with auth result {}",
        continuation, &auth_result
    );
    if continuation.contains('?') {
        Ok(Redirect::to(format!(
            "{}&result={}",
            continuation, auth_result
        )))
    } else {
        Ok(Redirect::to(format!(
            "{}?result={}",
            continuation, auth_result
        )))
    }
}

#[post("/start_authentication", data = "<request>")]
async fn start_authentication(
    config: State<'_, config::Config>,
    request: Json<StartAuthRequest>,
) -> Result<Json<StartAuthResponse>, Error> {
    config.verify_attributes(&request.attributes)?;

    let attributes = base64::encode_config(serde_json::to_vec(&request.attributes)?, URL_SAFE);
    let continuation = base64::encode_config(&request.continuation, URL_SAFE);

    if let Some(attr_url) = &request.attr_url {
        let attr_url = base64::encode_config(attr_url, URL_SAFE);

        Ok(Json(StartAuthResponse {
            client_url: format!(
                "{}/browser/{}/{}/{}",
                config.server_url(),
                attributes,
                continuation,
                attr_url,
            ),
        }))
    } else {
        Ok(Json(StartAuthResponse {
            client_url: format!(
                "{}/browser/{}/{}",
                config.server_url(),
                attributes,
                continuation,
            ),
        }))
    }
}

#[launch]
fn rocket() -> rocket::Rocket {
    rocket::ignite()
        .mount(
            "/",
            routes![start_authentication, user_inline, user_oob, session_update,],
        )
        .attach(AdHoc::config::<Config>())
}
