//! See: <https://github.com/pyrossh/rust-embed/blob/master/examples/axum.rs>.

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

#[derive(rust_embed::RustEmbed)]
#[folder = "static"]
struct Assets;

pub struct StaticFile<T: Into<String>>(pub T);

impl<T: Into<String>> IntoResponse for StaticFile<T> {
    fn into_response(self) -> Response {
        let path = self.0.into();

        match Assets::get(path.as_str()) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
}
