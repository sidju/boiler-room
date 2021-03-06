use crate::Error;
use crate::State;

// Time struct, for session timeout creation
use chrono::offset::Utc;
use chrono::Duration;

use shared_types::{Login, Session};

// Specifically designed login handler that behaves identically no matter
// if the account exists or not and if the password matches or not
//
// Take a post request with login data
// convert it into Some session key if valid
// if input was invalid returns none
pub async fn login(state: &'static State, form: Login) -> Result<Option<Session>, Error> {
  use futures::{pin_mut, select, FutureExt};
  use rand::Rng;

  // Wrap the execution in a delay
  // This should be longer than the processing time
  // so no changes in flow can affect execution time
  let delay =
    rand::thread_rng().gen_range(state.login_delay..(state.login_delay as f64 * 1.2) as u64);
  let delay = tokio::time::sleep(tokio::time::Duration::from_millis(delay)).fuse();
  // Call the inner handler
  let res = login_inner(state, form).fuse();

  // Select to receive the future which returns fastest
  pin_mut!(res);
  pin_mut!(delay);
  select! {
    // If the delay returns first we leak the processing time and all that implies
    _ = delay => {
      eprintln!("Login_delay insufficient! We are leaking the existence of accounts!");
      // Even so, behave as normally as we can, to not make this obvious
      Ok(res.await?)
    },
    res = res => {
      delay.await; // We want to wait the full delay, to obscure if we exited early
      Ok(res?)
    },
  }
}
// The simpler handler
// Using this directly will allow an attacker to see
// if users exist, since it exits early on failure
async fn login_inner(state: &'static State, form: Login) -> Result<Option<Session>, Error> {
  // Get the user from database
  // if none found, exit early
  let user = match sqlx::query!(
    "SELECT id,username,pass,locked,admin FROM users WHERE username = $1",
    form.username,
  )
  .fetch_optional(&state.db_pool)
  .await?
  {
    Some(user) => user,
    None => {
      return Ok(None);
    }
  };

  // If the password is nulled the user is deactivated
  let passhash = match user.pass {
    Some(x) => x,
    None => {
      return Ok(None);
    }
  };

  // If there is a user we check the hash
  match super::hash::verify(&state.cpu_semaphore, &state.hasher, passhash, form.password).await? {
    // Wrong password is not an error, but is is an early return
    false => {
      return Ok(None);
    }
    _ => (),
  };

  // Finally, check if the user account is locked
  if user.locked {
    return Err(Error::account_locked());
  }

  // If we get here we should create a random key
  // The risk of collision is around 1 in the number of atoms on earth
  // so don't even bother checking
  let key = nanoid::nanoid!(32);
  // Also create the deadline for the session, after which it becomes invalid
  let until = Utc::now().naive_utc()
    + if form.extended {
      Duration::days(1)
    } else {
      Duration::days(365)
    };

  // Make the database insert and return the session key
  let ret = sqlx::query_as!(
    Session,
    "
WITH s AS (
  INSERT INTO sessions(userid, key, until) VALUES($1, $2, $3)
  RETURNING id, userid, key, until
)
SELECT s.id, s.key, users.admin AS is_admin, users.username, s.until
FROM s
JOIN users
ON users.id = $1
    ",
    user.id,
    &key,
    &until,
  )
  .fetch_one(&state.db_pool)
  .await
  .map_err(|e| -> Error {
    match e {
      sqlx::Error::Database(ref err) => {
        if err.constraint() == Some("key") {
          Error::session_key_collision()
        } else {
          e.into()
        }
      }
      _ => e.into(),
    }
  })?;

  Ok(Some(ret))
}

// Small helper for invalidating session keys
// Note that you may need to delete it client side as well (cookies)
pub async fn logout(state: &'static State, key: Option<String>) -> Result<(), Error> {
  match key {
    Some(k) => {
      sqlx::query!("DELETE FROM sessions WHERE key = $1", k)
        .execute(&state.db_pool)
        .await
        ? // Converts error if needed
      ;
      Ok(())
    }
    None => Err(Error::unauthorized()),
  }
}
