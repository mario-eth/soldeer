//! Registry authentication
use crate::{
    errors::AuthError,
    registry::api_url,
    utils::{is_symlink, login_file_path},
};
use log::{debug, info, warn};
use reqwest::{
    Client, StatusCode,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, AuthError>;

/// Credentials to be used for login
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Response from the login endpoint
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoginResponse {
    pub status: String,
    /// JWT token
    pub token: String,
}

/// Get the JWT token from the environment or from the login file
///
/// Precedence is given to the `SOLDEER_API_TOKEN` environment variable.
pub fn get_token() -> Result<String> {
    if let Ok(token) = env::var("SOLDEER_API_TOKEN") &&
        !token.is_empty()
    {
        return Ok(token);
    }
    let token_path = login_file_path()?;
    let jwt =
        fs::read_to_string(&token_path).map_err(|_| AuthError::MissingToken)?.trim().to_string();
    if jwt.is_empty() {
        debug!(token_path:?; "token file exists but is empty");
        return Err(AuthError::MissingToken);
    }
    debug!(token_path:?; "token retrieved from file");
    Ok(jwt)
}

/// Get a header map with the bearer token set up if it exists
pub fn get_auth_headers() -> Result<HeaderMap> {
    let mut headers: HeaderMap = HeaderMap::new();
    let Ok(token) = get_token() else {
        return Ok(headers);
    };
    let header_value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| AuthError::InvalidToken)?;
    headers.insert(AUTHORIZATION, header_value);
    Ok(headers)
}

/// Save an access token in the login file
pub fn save_token(token: &str) -> Result<PathBuf> {
    let token_path = login_file_path()?;
    write_token(&token_path, token)?;
    Ok(token_path)
}

/// Write the token to the login file without following symlinks.
///
/// The token is written to a freshly created temporary file in the same folder,
/// which is then renamed to overwrite the final location.
fn write_token(path: &Path, token: &str) -> Result<()> {
    if is_symlink(path)? {
        return Err(AuthError::IOError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "login file must not be a symlink",
        )));
    }
    let Some(filename) = path.file_name() else {
        return Err(AuthError::IOError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "login file path must point to a file",
        )));
    };
    let mut tmp_filename = filename.to_os_string();
    tmp_filename.push(format!(".{}.tmp", Uuid::new_v4()));
    let tmp_path = path.with_file_name(tmp_filename);
    let res = create_private_file(&tmp_path, token).and_then(|()| fs::rename(&tmp_path, path));
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

/// Create a new file readable and writable only by its owner and write the contents into it.
///
/// The file must not exist already, which guarantees that no symlink is followed.
fn create_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // the creation mode is masked by the process umask, so enforce the
        // permissions on the open handle
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents.as_bytes())
}

/// Retrieve user profile for the token to check its validity, returning the username
pub async fn check_token(token: &str) -> Result<String> {
    let client = Client::new();
    let url = api_url("v1", "auth/validate-cli-token", &[]);
    let mut headers: HeaderMap = HeaderMap::new();
    let header_value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| AuthError::InvalidToken)?;
    headers.insert(AUTHORIZATION, header_value);
    let response = client.get(url).headers(headers).send().await?;
    match response.status() {
        s if s.is_success() => {
            #[derive(Deserialize)]
            struct User {
                id: String,
                username: String,
            }
            #[derive(Deserialize)]
            struct UserResponse {
                data: User,
            }
            let res: UserResponse = response.json().await?;
            debug!("token is valid for user {} with ID {}", res.data.username, res.data.id);
            Ok(res.data.username)
        }
        StatusCode::UNAUTHORIZED => Err(AuthError::InvalidToken),
        _ => Err(AuthError::HttpError(
            response.error_for_status().expect_err("result should be an error"),
        )),
    }
}

/// Execute the login request and store the JWT token in the login file
pub async fn execute_login(login: &Credentials) -> Result<PathBuf> {
    warn!(
        "the option to login via email and password will be removed in a future version of Soldeer. Please update your usage by either using `soldeer login --token [YOUR CLI TOKEN]` or passing the `SOLDEER_API_TOKEN` environment variable to the `push` command."
    );

    let token_path = login_file_path()?;
    let url = api_url("v1", "auth/login", &[]);
    let client = Client::new();
    let res = client.post(url).json(login).send().await?;
    match res.status() {
        s if s.is_success() => {
            debug!("login request completed");
            let response: LoginResponse = res.json().await?;
            write_token(&token_path, &response.token)?;
            info!(token_path:?; "login successful");
            Ok(token_path)
        }
        StatusCode::UNAUTHORIZED => Err(AuthError::InvalidCredentials),
        _ => Err(AuthError::HttpError(
            res.error_for_status().expect_err("result should be an error"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temp_env::{async_with_vars, with_var};
    use testdir::testdir;

    #[tokio::test]
    async fn test_login_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"200","token":"jwt_token_example"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(fs::canonicalize(res.unwrap()).unwrap(), fs::canonicalize(&test_file).unwrap());
        assert_eq!(fs::read_to_string(test_file).unwrap(), "jwt_token_example");
    }

    #[tokio::test]
    async fn test_login_401() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"401"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(matches!(res, Err(AuthError::InvalidCredentials)), "{res:?}");
    }

    #[tokio::test]
    async fn test_login_500() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"500"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(matches!(res, Err(AuthError::HttpError(_))), "{res:?}");
    }

    #[tokio::test]
    async fn test_check_token_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/auth/validate-cli-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"status":"success","data":{"created_at": "2024-08-04T14:21:31.622589Z","email": "test@test.net","id": "b6d56bf0-00a5-474f-b732-f416bef53e92","organization": "test","role": "owner","updated_at": "2024-08-04T14:21:31.622589Z","username": "test","verified": true}}"#,
            )
            .create_async()
            .await;

        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], check_token("eyJ0..."))
                .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "test");
    }

    #[tokio::test]
    async fn test_check_token_failure() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/auth/validate-cli-token")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"fail","message":"Invalid token"}"#)
            .create_async()
            .await;

        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], check_token("foobar")).await;
        assert!(res.is_err(), "{res:?}");
    }

    #[test]
    fn test_get_token_env() {
        let res = with_var("SOLDEER_API_TOKEN", Some("test"), get_token);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "test");
    }

    /// Save a token into `path`, with `SOLDEER_LOGIN_FILE` pointing at it.
    fn save_token_to(path: &Path, token: &str) -> Result<PathBuf> {
        temp_env::with_vars(
            [("SOLDEER_LOGIN_FILE", Some(path.to_string_lossy().to_string()))],
            || save_token(token),
        )
    }

    #[cfg(unix)]
    #[test]
    fn test_save_token_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let file = testdir!().join("token");
        let saved = save_token_to(&file, "secret").unwrap();

        assert_eq!(saved, file);
        assert_eq!(fs::read_to_string(&file).unwrap(), "secret");
        assert_eq!(fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn test_save_token_tightens_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = testdir!();
        let file = dir.join("token");
        fs::write(&file, "old").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

        save_token_to(&file, "secret").unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "secret");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        // the temporary file must not be left behind
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_save_token_rejects_symlink() {
        let dir = testdir!();
        let target = dir.join("target");
        fs::write(&target, "original").unwrap();
        let link = dir.join("token");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let res = save_token_to(&link, "secret");
        assert!(res.is_err(), "{res:?}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    }
}
