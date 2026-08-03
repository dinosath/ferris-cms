//! Login/Register screen specs (design Part VII §1-§2).

use serde::{Deserialize, Serialize};

/// Login screen state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoginScreen {
    pub email: String,
    pub password: String,
    pub error: Option<String>,
    pub loading: bool,
}

/// Register screen state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegisterScreen {
    pub firstname: String,
    pub lastname: String,
    pub email: String,
    pub password: String,
    pub confirm_password: String,
    pub error: Option<String>,
    pub loading: bool,
}

/// Auth route — which screen to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthScreen {
    Login,
    Register,
}
