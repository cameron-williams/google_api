use std::env;

use std::fs::{create_dir_all, File, OpenOptions};
use std::io::prelude::*;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use reqwest::blocking::multipart::Form;
use reqwest::Url;

use log;

use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::thread;


const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:3000/";
const DEFAULT_DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
const DRIVE_BASE_URL: &str = "https://www.googleapis.com/drive/v3";
const CONFIG_PATH: &str = "/.config/cameron-williams/google_api";

fn config_dir() -> PathBuf {
    let mut dir = env::var("HOME").expect("$HOME not set");
    dir.push_str(CONFIG_PATH);
    PathBuf::from(dir)
}

/// Will be stored locally in .config or something. Google OAuth savedata.
#[derive(Debug, Deserialize, Serialize)]
struct GoogleOAuthToken {
    token: String,
    expires: SystemTime,
    code: String,
    refresh_token: String,
    scope: Vec<String>,
}

impl GoogleOAuthToken {
    fn new() -> GoogleOAuthToken {
        GoogleOAuthToken {
            token: String::new(),
            expires: SystemTime::now(),
            code: String::new(),
            refresh_token: String::new(),
            scope: Vec::new(),
        }
    }

    /// Try to get existing GoogleOAuthToken from config path. Or return a new one.
    fn from_config(client_id: &String, client_secret: &String) -> Result<GoogleOAuthToken, String> {
        let mut auth = Self::read_from_path()?;
        auth.ensure_validity(client_id, client_secret)?;
        Ok(auth)
    }

    /// Ensures the validity of the current GoogleOAuthToken. Will return Err if unable to create a valid token.
    fn ensure_validity(
        &mut self,
        client_id: &String,
        client_secret: &String,
    ) -> Result<(), String> {
        // No code == we need to get user consent.
        if self.code.is_empty() {
            log::debug!("no consent");
            self.prompt_user_authentication(client_id)?;
        }

        // Check that we even have a valid token from user auth.
        if self.refresh_token.is_empty() {
            log::debug!("no token");
            self.get_new_access_token(client_id, client_secret)?;
        }

        // Check if token is expired.
        if self.expires < SystemTime::now() {
            log::debug!("token expired");
            self.refresh_access_token(client_id, client_secret)?
        }
        Ok(())
    }

    /// Prompt the user to approve the app's access for whatever scopes are currently required.
    fn prompt_user_authentication(&mut self, client_id: &String) -> Result<(), String> {
        // Build auth url (use default values for now).
        let auth_url = Url::parse_with_params(
            GOOGLE_AUTH_URL,
            &[
                ("client_id", client_id.as_str()),
                ("redirect_uri", DEFAULT_REDIRECT_URI),
                ("response_type", "code"),
                ("scope", DEFAULT_DRIVE_SCOPE),
            ],
        )
        .unwrap();

        self.scope = vec![String::from(DEFAULT_DRIVE_SCOPE)];

        // Open auth url in user's browser for their consent.
        if let Err(e) = open::that(auth_url.as_str()) {
            return Err(format!(
                "failed to open google consent url in browser: {:#?}",
                e
            ));
        }

        log::info!("This application needs your consent to use Google Drive. Please check your browser and either approve or deny it.");

        // Create a listener that waits until we get a response from the user's consent.
        let (tx, rx) = channel();
        thread::spawn(move || {
            let listener = TcpListener::bind("127.0.0.1:3000").unwrap();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buffer = [0; 512];
                stream.read(&mut buffer).unwrap();
                stream
                    .write("HTTP/1.1 200 OK\r\n\r\n<body><p>Authenticated.</p></body>".as_bytes())
                    .unwrap();
                stream.flush().unwrap();
                tx.send(buffer).unwrap();
            }
        });

        // Receiver response or timeout after 45s. Parse response url to get status params.
        let response_url = match rx.recv_timeout(Duration::from_secs(45)) {
            Ok(r) => {
                let mut base_url = String::from("http://127.0.0.1");
                let body = String::from_utf8_lossy(&r[..]);
                let mut split = body.split_whitespace();
                // Discard first index of split body.
                split.next();
                // 2nd index is the response url which we need to get the code from.
                base_url.push_str(split.next().unwrap());
                Url::parse(&base_url).unwrap()
            }
            Err(e) => {
                // error!("error getting auth: {}", e);
                return Err(format!(
                    "failed to get authentication from user consent: {}",
                    e
                ));
            }
        };

        // Check if our response url is valid or err.
        let status = response_url.query_pairs().next().unwrap();
        if status.0 == "error" {
            return Err(status.1.to_string());
        } else {
            self.code = status.1.to_string();
        }

        // Write new token to config file.
        self.write_to_path()?;

        Ok(())
    }

    /// Get a new access token from Google using saved consent code.
    fn get_new_access_token(
        &mut self,
        client_id: &String,
        client_secret: &String,
    ) -> Result<(), String> {
        let form = Form::new()
            .text("code", self.code.clone())
            .text("client_id", client_id.clone())
            .text("client_secret", client_secret.clone())
            .text("redirect_uri", DEFAULT_REDIRECT_URI)
            .text("grant_type", "authorization_code");
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(GOOGLE_TOKEN_URL)
            .header("charset", "utf-8")
            .multipart(form)
            .send();
        let resp: serde_json::Value = match resp {
            Ok(r) => r.json().unwrap(),
            Err(e) => {
                return Err(format!(
                    "response error on new token request post: {:#?}",
                    e
                ))
            }
        };
        // Update self values from response.
        self.refresh_token = String::from(resp["refresh_token"].as_str().unwrap());
        self.token = String::from(resp["access_token"].as_str().unwrap());
        self.expires =
            SystemTime::now() + Duration::from_secs(resp["expires_in"].as_u64().unwrap());

        // Write current config to path.
        self.write_to_path()
    }

    // Refresh our current access token using saved consent code/refresh token.
    fn refresh_access_token(
        &mut self,
        client_id: &String,
        client_secret: &String,
    ) -> Result<(), String> {
        let form = Form::new()
            .text("client_id", client_id.clone())
            .text("client_secret", client_secret.clone())
            .text("refresh_token", self.refresh_token.clone())
            .text("grant_type", "refresh_token");
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(GOOGLE_TOKEN_URL)
            .header("charset", "utf-8")
            .multipart(form)
            .send();
        let resp: serde_json::Value = match resp {
            Ok(r) => r.json().unwrap(),
            Err(e) => return Err(format!("response err on refresh token request: {:#?}", e)),
        };

        // Update token and expire values from response.
        self.token = String::from(resp["access_token"].as_str().unwrap());
        self.expires =
            SystemTime::now() + Duration::from_secs(resp["expires_in"].as_u64().unwrap());
        // Write updated auth to config.
        self.write_to_path()
    }

    /// Eventually allow for custom config path.
    fn read_from_path() -> Result<GoogleOAuthToken, String> {
        let path = config_dir();
        // Ensure config path exists. If it doesn't create it and return a blank GoogleOAuthToken.
        if !path.exists() {
            match create_dir_all(path.parent().unwrap()) {
                Ok(_) => {
                    if let Err(e) = File::create(&path) {
                        return Err(format!("failed to create new config file: {:#?}", e));
                    }
                }
                Err(e) => return Err(format!("failed to create config dir: {:#?}", e)),
            }
            return Ok(GoogleOAuthToken::new());
        }
        match OpenOptions::new()
            .read(true)
            .write(false)
            .open(config_dir())
        {
            Ok(f) => {
                let reader = BufReader::new(f);
                match serde_json::from_reader(reader) {
                    Ok(d) => Ok(d),
                    Err(_) => Ok(GoogleOAuthToken::new()),
                }
            }
            Err(e) => Err(format!("error reading from config file: {:#?}", e)),
        }
    }

    /// Write the current GoogleOAuthToken state to the config file.
    /// Eventually add parameter for custom config path.
    fn write_to_path(&self) -> Result<(), String> {
        // Ensure config path exists. If it doesn't create it.
        let path = config_dir();
        if !path.exists() {
            match create_dir_all(path.parent().unwrap()) {
                Ok(_) => {
                    if let Err(e) = File::create(&path) {
                        return Err(format!("failed to create new config file: {:#?}", e));
                    }
                }
                Err(e) => return Err(format!("failed to create config dir: {:#?}", e)),
            }
        }
        // Write current state of self GoogleOAuthToken to file as json.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .open(path)
        {
            Ok(f) => {
                let writer = BufWriter::new(f);
                if let Err(e) = serde_json::to_writer_pretty(writer, &self) {
                    return Err(format!(
                        "error writing/serializing config to file: {:#?}",
                        e
                    ));
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(format!("error opening config file in write mode: {:#?}", e)),
        }
    }
}

#[derive(Debug)]
pub struct Drive {
    auth: GoogleOAuthToken,
    client: reqwest::blocking::Client,
}

impl Drive {
    pub fn new(
        client_id: String,
        client_secret: String,
        scopes: Option<Vec<String>>,
    ) -> Result<Drive, String> {
        // doesn't do anything currently, eventually allow manually specified scopes.
        let _scopes: Vec<String> = if let None = scopes {
            vec![String::from(DEFAULT_DRIVE_SCOPE)]
        } else {
            scopes.unwrap()
        };

        Ok(Drive {
            auth: GoogleOAuthToken::from_config(&client_id, &client_secret)?,
            client: reqwest::blocking::Client::new(),
        })
    }

    pub fn get(
        &self,
        endpoint: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        // Build url with optional params.
        let mut url = String::from(DRIVE_BASE_URL);
        url.push_str(endpoint);
        let url = match params {
            Some(p) => Url::parse_with_params(&url, p).unwrap(),
            None => Url::parse(&url).unwrap(),
        };

        self.client
            .get(url)
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .send()
    }

    pub fn post(
        &self,
        endpoint: &str,
        params: Option<Vec<(&str, &str)>>,
        json: serde_json::Value,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        // Build url with optional params.
        let mut url = String::from(DRIVE_BASE_URL);
        url.push_str(endpoint);
        let url = match params {
            Some(p) => Url::parse_with_params(&url, p).unwrap(),
            None => Url::parse(&url).unwrap(),
        };

        self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .json(&json)
            .send()
    }

    pub fn patch(
        &self,
        endpoint: &str,
        params: Option<Vec<(&str, &str)>>,
        json: serde_json::Value,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        // Build url with optional params.
        let mut url = String::from(DRIVE_BASE_URL);
        url.push_str(endpoint);
        let url = match params {
            Some(p) => Url::parse_with_params(&url, p).unwrap(),
            None => Url::parse(&url).unwrap(),
        };

        self.client
            .patch(url)
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .json(&json)
            .send()
    }

    pub fn delete(
        &self,
        endpoint: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        // Build url with optional params.
        let mut url = String::from(DRIVE_BASE_URL);
        url.push_str(endpoint);
        let url = match params {
            Some(p) => Url::parse_with_params(&url, p).unwrap(),
            None => Url::parse(&url).unwrap(),
        };

        self.client
            .delete(url)
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .send()
    }

    fn get_file_id_from_url(url: &str) -> Result<String, String> {
        // Get file id from passed url.
        let url = Url::parse(url).expect("invalid url format");
        match url.query_pairs().next() {
            Some(i) => Ok(String::from(i.1)),
            None => Err(String::from("no url param specified")),
        }
    }

    /// Get given file's metadata from Drive for given url.
    pub fn file_metadata(&self, url: &str) -> Result<serde_json::Value, reqwest::Error> {
        let id = Drive::get_file_id_from_url(url).expect("no id param in given url");

        let resp = self
            .get(format!("/files/{}?fields=*", id).as_str(), None)?
            .json()?;

        Ok(resp)
    }

    /// Download file from given drive url to given path. Return the path the file was downloaded to.
    pub fn download_file(&self, url: &str, path: PathBuf) -> Result<PathBuf, reqwest::Error> {
        // Get file id from passed url.
        let id = Drive::get_file_id_from_url(url).expect("no id param in given url");

        // If path is a dir, get the name of the file we are downloading and add it to the path.
        let path = if path.is_dir() {
            let v = self.file_metadata(url)?;
            let mut p = PathBuf::from(path);
            p.push(v["name"].as_str().unwrap());
            p
        } else {path};

        // Get the file from Drive and put to buffer.
        let mut resp = self.get(
            format!("/files/{}", id).as_str(),
            Some(vec![("alt", "media")]),
        )?;
        let mut buf: Vec<u8> = vec![];
        resp.copy_to(&mut buf)?;

        // Write file locally.
        let mut file = File::create(&path).unwrap();
        file.write_all(&buf).expect("failed to write buf to file");

        Ok(path)
    }

    /// Upload file at given path to Google Drive. Todo:// make it one request somehow?
    pub fn upload_file(&self, path: &PathBuf) -> Result<String, reqwest::Error> {
        // Google Drive file upload url has a different base url.
        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
        let file = File::open(&path).expect("failed to open file for upload");
        let resp: serde_json::Value = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .body(file)
            .send()?
            .json()?;
        let file_id = resp["id"].as_str().unwrap().clone();
        let url = format!("https://drive.google.com/open?id={}", file_id);
        // Patch to update file name with one from given path.
        self.patch(
            format!("/files/{}", file_id).as_str(),
            None,
            serde_json::json!({"name": path.file_name().unwrap().to_str()}),
        )?;
        Ok(url)
    }

    /// Update file at given drive url from local file path.reqwest
    pub fn update_file(&self, path: PathBuf, url: &str) -> Result<(), reqwest::Error> {
        // Google drive file update url has a different base url.
        let file_id = Drive::get_file_id_from_url(url).unwrap();
        let fmt_url = format!(
            "https://www.googleapis.com/upload/drive/v3/files/{}?uploadType=multipart",
            file_id
        );
        let file = File::open(&path).expect("failed to open local file for update");

        self.client
            .patch(fmt_url.as_str())
            .header("Authorization", format!("Bearer {}", &self.auth.token))
            .body(file)
            .send()?;
            // .json()?;
        Ok(())
    }

    /// Delete file at given drive url.
    pub fn delete_file(&self, url: &str) -> Result<(), reqwest::Error> {
        let file_id = Drive::get_file_id_from_url(url).unwrap();
        let endpoint = format!("/files/{}", file_id);
        self.delete(endpoint.as_str(), None)?;
        Ok(())
    }
}
