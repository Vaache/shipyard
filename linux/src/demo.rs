use crate::api::models::RestUser;
use crate::brand;

pub fn repo() -> Option<String> {
    brand::env_var("DEMO_REPO")
}

pub fn folder() -> Option<String> {
    brand::env_var("DEMO_FOLDER")
}

pub fn is_on() -> bool {
    repo().is_some() || folder().is_some()
}

pub fn user() -> RestUser {
    RestUser { login: "arivera".into(), name: Some("Alex Rivera".into()), avatar_url: None }
}

pub const EMAIL: &str = "alex@example.com";

pub fn query(base: &str) -> String {
    match repo() {
        Some(r) => format!("repo:{r}"),
        None => base.to_string(),
    }
}
