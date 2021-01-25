use crate::auth::Auth;
use crate::options::EmailVerification;
use crate::util::normalise_email;
use crate::util::{Error, Result};

use argon2::Config;
use nanoid::nanoid;
use rocket::State;
use mongodb::bson::{Bson, doc};
use rocket_contrib::json::{Json, JsonValue};
use ulid::Ulid;
use validator::Validate;
use serde::Deserialize;
use chrono::Utc;

lazy_static! {
    static ref ARGON_CONFIG: Config<'static> = Config::default();
}

#[derive(Debug, Validate, Deserialize)]
pub struct Create {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8, max = 72))]
    password: String,
}

impl Auth {
    pub async fn create_account(&self, data: Create) -> Result<String> {
        data.validate()
            .map_err(|error| Error::FailedValidation { error })?;

        let normalised = normalise_email(data.email.clone());

        if self
            .collection
            .find_one(
                doc! {
                    "email_normalised": &normalised
                },
                None,
            )
            .await
            .map_err(|_| Error::DatabaseError)?
            .is_some()
        {
            return Err(Error::EmailInUse);
        }

        let hash = argon2::hash_encoded(
            data.password.as_bytes(),
            Ulid::new().to_string().as_bytes(),
            &ARGON_CONFIG,
        )
        .map_err(|_| Error::InternalError)?;

        let verification = if let EmailVerification::Enabled { verification_expiry, verification_ratelimit, .. } = self.options.email_verification {
            let token = nanoid!(32);

            doc! {
                "type": "Pending",
                "token": token,
                "expiry": Bson::DateTime(Utc::now() + verification_expiry),
                "rate_limit": Bson::DateTime(Utc::now() + verification_ratelimit)
            }
        } else {
            doc! {
                "type": "Verified"
            }
        };
        
        let user_id = Ulid::new().to_string();
        self.collection
            .insert_one(
                doc! {
                    "_id": &user_id,
                    "email": &data.email,
                    "email_normalised": normalised,
                    "password": hash,
                    "verification": verification,
                    "sessions": []
                },
                None,
            )
            .await
            .map_err(|_| Error::DatabaseError)?;

        Ok(user_id)
    }
}

#[post("/create", data = "<data>")]
pub async fn create_account(
    auth: State<'_, Auth>,
    data: Json<Create>,
) -> crate::util::Result<JsonValue> {
    Ok(json!({
        "user_id": auth.inner().create_account(data.into_inner()).await?
    }))
}
