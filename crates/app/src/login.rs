//! The plain-stdout device-flow login shared by `login` and the standalone
//! first run (spec 0002, "Commands").

use std::io::Write;
use std::time::Duration;

use tidal_player_api::auth::{
    AuthConfig, DeviceCode, DeviceFlow, LOSSY_WARNING, Login, RefreshClient, SessionStore,
};

/// How a login attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginOutcome {
    LoggedIn,
    Failed,
    Cancelled,
}

impl LoginOutcome {
    /// Process exit code: 0, 1, or 130 for Ctrl-C.
    pub fn exit_code(self) -> u8 {
        match self {
            Self::LoggedIn => 0,
            Self::Failed => 1,
            Self::Cancelled => 130,
        }
    }
}

/// Whole minutes for the prompt, rounded up.
pub fn expiry_minutes(expires_in: Duration) -> u64 {
    expires_in.as_secs().div_ceil(60)
}

/// The prompt printed before waiting for approval.
pub fn format_prompt(link: &str, user_code: &str, minutes: u64) -> String {
    format!(
        "To log in, open this link and approve the code:\n\n  {link}\n\n\
         Code: {user_code}   (expires in {minutes} min)\n\
         Waiting for approval… (Ctrl-C to cancel)\n"
    )
}

/// Runs the device flow, saving the session to `store` on success. Prints
/// to stdout/stderr; nothing is stored on failure or Ctrl-C.
pub async fn run_login(store: &dyn SessionStore) -> LoginOutcome {
    let flow = match DeviceFlow::new(AuthConfig::production()) {
        Ok(flow) => flow,
        Err(e) => return fail(&e),
    };
    let code: DeviceCode = match flow.start_device_flow().await {
        Ok(code) => code,
        Err(e) => return fail(&e),
    };
    println!(
        "{}",
        format_prompt(
            &code.login_link(),
            &code.user_code,
            expiry_minutes(code.expires_in)
        )
    );
    let _ = std::io::stdout().flush();
    let browser_link = code.browser_link();
    // Opening may block (text browsers), and failure is ignored.
    std::thread::spawn(move || {
        let _ = webbrowser::open(&browser_link);
    });
    tokio::select! {
        result = flow.complete_login(&code, store) => match result {
            Ok(Login { session, client }) => {
                if client == RefreshClient::DeviceFlow {
                    eprintln!("Warning: {LOSSY_WARNING}");
                }
                println!("Logged in as user {} ({})", session.user_id, session.country_code);
                LoginOutcome::LoggedIn
            }
            Err(e) => fail(&e),
        },
        _ = tokio::signal::ctrl_c() => {
            eprintln!();
            LoginOutcome::Cancelled
        }
    }
}

fn fail(e: &dyn std::fmt::Display) -> LoginOutcome {
    eprintln!("{e}");
    LoginOutcome::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_matches_spec() {
        assert_eq!(
            format_prompt("https://link.tidal.com", "ABCDE", 5),
            "To log in, open this link and approve the code:\n\n  https://link.tidal.com\n\n\
             Code: ABCDE   (expires in 5 min)\nWaiting for approval… (Ctrl-C to cancel)\n"
        );
        assert_eq!(expiry_minutes(Duration::from_secs(300)), 5);
        assert_eq!(expiry_minutes(Duration::from_secs(301)), 6);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(LoginOutcome::LoggedIn.exit_code(), 0);
        assert_eq!(LoginOutcome::Failed.exit_code(), 1);
        assert_eq!(LoginOutcome::Cancelled.exit_code(), 130);
    }
}
