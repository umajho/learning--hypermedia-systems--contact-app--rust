use std::sync::Arc;

use axum::{
    extract::Request,
    middleware::Next,
    response::{Html, Response},
};
use axum_flash::IncomingFlashes;

#[derive(Clone)]
pub struct Layouter(
    pub Arc<dyn Fn(IncomingFlashes, markup::DynRender) -> Html<String> + Send + Sync + 'static>,
);

pub async fn with_layouter(mut req: Request, next: Next) -> Response {
    let layouter = Layouter(Arc::new(|flashes, content| {
        Html(layouts::Default { flashes, content }.to_string())
    }));

    req.extensions_mut().insert(layouter);

    next.run(req).await
}

mod layouts {
    use axum_flash::IncomingFlashes;

    markup::define! {
      Default<T: markup::Render>(flashes: IncomingFlashes, content: T) {
            @markup::doctype()
            html {
                head {
                    script [
                        src="https://unpkg.com/htmx.org@1.9.9",
                        integrity="sha384-QFjmbokDn2DjBjq+fM+8LUIVrAgqcNW2s0PjAxHETgRn9l4fvX31ZxDxvwQnyMOX",
                        crossorigin="anonymous",
                    ] {}
                    title { "Contact App" }
                    link [rel="stylesheet", href="https://unpkg.com/missing.css@1.1.1"];
                    link [rel="stylesheet", href="/static/site.css"];
                }
            }
            body ["hx-boost"="true"] {
                main {
                    @for (_, message) in flashes.iter() {
                        div .flash { @message }
                    }
                    @content
                }
            }
        }
    }
}
