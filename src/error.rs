//! This both contains a custom rejection for returning when there may be another route matching
//! and an error for when the current route is the correct one but something is wrong

use crate::Reply;
use hyper::{Body, Response, Method, StatusCode};
use argon2::password_hash;
use serde::Serialize;

#[derive(Debug)]
pub enum Error {
  // First routing errors
  PathNotFound(String),
  MethodNotFound(Method),
  BadRequest(String),
  Unauthorized,
  Forbidden,

  // Parsing errors
  InvalidHeader(hyper::header::ToStrError),
  InvalidJson(serde_json::Error),
  InvalidUrlEncoding(serde_urlencoded::de::Error),

  // Errors from internal input validation
  BadUsername(String),
  BadPassword,
  UsernameTaken,
  BadLogin,

  // Then wrapped errors that all equate an internal error when returned
  SessionKeyCollision, // Technically possible, but insanely unlikely
  Join(tokio::task::JoinError),
  Semaphore(tokio::sync::AcquireError),
  Hash(password_hash::Error),
  Db(sqlx::Error),
  ConnectionError(hyper::Error),
}

// To enable using '?' we implement from for the wrapped errors
impl From<tokio::task::JoinError> for Error {
  fn from(e: tokio::task::JoinError) -> Self {
    Self::Join(e)
  }
}
impl From<tokio::sync::AcquireError> for Error {
  fn from(e: tokio::sync::AcquireError) -> Self {
    Self::Semaphore(e)
  }
}
impl From<password_hash::Error> for Error {
  fn from(e: password_hash::Error) -> Self {
    Self::Hash(e)
  }
}
impl From<sqlx::Error> for Error {
  fn from(e: sqlx::Error) -> Self {
    Self::Db(e)
  }
}
impl From<serde_json::Error> for Error {
  fn from(e: serde_json::Error) -> Self {
    Self::InvalidJson(e)
  }
}
impl From<serde_urlencoded::de::Error> for Error {
  fn from(e: serde_urlencoded::de::Error) -> Self {
    Self::InvalidUrlEncoding(e)
  }
}
impl From<hyper::header::ToStrError> for Error {
  fn from(e: hyper::header::ToStrError) -> Self {
    Self::InvalidHeader(e)
  }
}
impl From<hyper::Error> for Error {
  fn from(e: hyper::Error) -> Self {
    Self::ConnectionError(e)
  }
}

// Make errors autoconvert into a consistent and descriptive reply
#[derive(Serialize)]
enum JsonError {
  InternalError,
  PathNotFound(String),
  MethodNotFound(String),
  BadRequest(String),
  Unauthorized,
  Forbidden,

  InvalidJson(String),
  InvalidUrlEncoding(String),
  InvalidHeader(String),

  BadUsername(String),
  BadPassword,
  BadLogin,
  UsernameTaken,
}
impl Into<Body> for JsonError {
  fn into(self) -> Body {
    serde_json::to_string(&self).unwrap().into()
  }
}

impl Reply for Error {
  fn into_response(self) -> Response<Body> {
    let (status,body) = match self {
      // Routing errors
      Self::PathNotFound(s) => {
        (StatusCode::NOT_FOUND, JsonError::PathNotFound(s))
      },
      Self::MethodNotFound(m) => {
        (StatusCode::METHOD_NOT_ALLOWED, JsonError::MethodNotFound( format!("{}", m) ))
      },
      Self::BadRequest(s) => {
        (StatusCode::BAD_REQUEST, JsonError::BadRequest(s))
      },
      Self::Unauthorized => {
        (StatusCode::UNAUTHORIZED, JsonError::Unauthorized)
      },
      Self::Forbidden => {
        (StatusCode::FORBIDDEN, JsonError::Forbidden)
      },

      // Input formatting errors
      Self::InvalidJson(e) => {
        (StatusCode::BAD_REQUEST, JsonError::InvalidJson(format!("{}", e)))
      },
      Self::InvalidUrlEncoding(e) => {
        (StatusCode::BAD_REQUEST, JsonError::InvalidUrlEncoding(format!("{}", e)))
      },
      Self::InvalidHeader(e) => {
        (StatusCode::BAD_REQUEST, JsonError::InvalidHeader(format!("{}", e)))
      },

      // User input errors
      Self::BadUsername(s) => {
        (StatusCode::BAD_REQUEST, JsonError::BadUsername(s))
      },
      Self::BadPassword => {
        (StatusCode::BAD_REQUEST, JsonError::BadPassword)
      },
      Self::UsernameTaken => {
        (StatusCode::BAD_REQUEST, JsonError::UsernameTaken)
      },
      Self::BadLogin => {
        (StatusCode::UNAUTHORIZED, JsonError::BadLogin)
      },

      // Internal errors (which need logging)
      Self::SessionKeyCollision => {
        eprintln!("Session key collision occured. If this happens twice something is wrong.");
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
      Self::Join(err) => {
        eprintln!("Error joining blocking task. {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
      Self::Semaphore(err) => {
        eprintln!("Error getting semaphore. {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
      Self::Hash(err) => {
        eprintln!("Error hashing password. {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
      Self::Db(err) => {
        eprintln!("Database error. {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
      Self::ConnectionError(err) => {
        eprintln!("Connection error. {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, JsonError::InternalError)
      },
    };
    let mut re = Response::new(body.into());
    *re.status_mut() = status;
    re
  }
}