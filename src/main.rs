mod contact_model;
mod contact_repo;

use std::sync::Arc;

use axum::{
    extract::{Form, FromRef, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use axum_flash::{Flash, IncomingFlashes};
use serde::Deserialize;
use sqlx::sqlite::SqlitePoolOptions;
use tower_http::{catch_panic::CatchPanicLayer, services::ServeDir};

use contact_model::{Contact, ContactErrors, ContactId};
use contact_repo::ContactRepo;

#[derive(Clone)]
struct AppState {
    /// FIXME: not working for Safari (on localhost?).
    flash_config: axum_flash::Config,

    contacts: Arc<ContactRepo>,
}
impl FromRef<AppState> for axum_flash::Config {
    fn from_ref(state: &AppState) -> Self {
        state.flash_config.clone()
    }
}

#[tokio::main]
async fn main() {
    let pool = SqlitePoolOptions::new().connect(":memory:").await.unwrap();

    let flash_config = axum_flash::Config::new(axum_flash::Key::generate());
    let contacts = Arc::new(ContactRepo::build_with_fake_data(pool, 3).await.unwrap());
    let app_state = AppState {
        flash_config,
        contacts,
    };

    let app = Router::new()
        .nest_service("/static", ServeDir::new("static"))
        .route("/", get(root))
        .route("/contacts", get(contacts_get))
        .route("/contacts/new", get(contacts_new_get))
        .route("/contacts/new", post(contacts_new_post))
        .route("/contacts/:contact_id", get(contacts_view_get))
        .route("/contacts/:contact_id/edit", get(contacts_edit_get))
        .route("/contacts/:contact_id/edit", post(contacts_edit_post))
        .route("/contacts/:contact_id/delete", post(contacts_delete_post))
        .layer(CatchPanicLayer::new())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> impl IntoResponse {
    Redirect::to("/contacts")
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn contacts_get(
    State(app_state): State<AppState>,
    flashes: IncomingFlashes,
    search: Option<Query<SearchQuery>>,
) -> impl IntoResponse {
    let q = search.map(|q| q.q.trim().to_string());
    let contacts_set = match &q {
        Some(q) if !q.is_empty() => app_state.contacts.search(q).await,
        _ => app_state.contacts.all().await,
    }
    .unwrap();

    let rendered = Layout {
        flashes: &flashes,
        content: ContactsContent {
            contacts: contacts_set,
            q,
        },
    }
    .to_string();
    (flashes, Html(rendered))
}

async fn contacts_new_get(flashes: IncomingFlashes) -> impl IntoResponse {
    let rendered = Layout {
        flashes: &flashes,
        content: NewContactContent {
            contact: None,
            errors: None,
        },
    }
    .to_string();
    (flashes, Html(rendered))
}

#[derive(Deserialize)]
struct NewContactForm {
    first_name: String,
    last_name: String,
    phone: String,
    email: String,
}
impl NewContactForm {
    fn build_contact(self, id: ContactId) -> Contact {
        Contact::builder()
            .id(id)
            .first(self.first_name)
            .last(self.last_name)
            .phone(self.phone)
            .email(self.email)
            .build()
    }
}

async fn contacts_new_post(
    State(app_state): State<AppState>,
    flashes: IncomingFlashes,
    flash: Flash,
    Form(form): Form<NewContactForm>,
) -> Response {
    let contact = form.build_contact(app_state.contacts.pop_id());

    match app_state.contacts.save(&contact).await.unwrap() {
        Ok(_) => (
            flash.success("Created New Contact!"),
            Redirect::to("/contacts"),
        )
            .into_response(),
        Err(errors) => {
            let rendered = Layout {
                flashes: &flashes,
                content: NewContactContent {
                    contact: Some(&contact),
                    errors: Some(errors),
                },
            }
            .to_string();
            (flashes, Html(rendered)).into_response()
        }
    }
}

async fn contacts_view_get(
    State(app_state): State<AppState>,
    flashes: IncomingFlashes,
    Path(contact_id): Path<String>,
) -> impl IntoResponse {
    let contact = app_state
        .contacts
        .find(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap()
        .unwrap();

    let rendered = Html(
        Layout {
            flashes: &flashes,
            content: ViewContactContent { contact: &contact },
        }
        .to_string(),
    );
    (flashes, rendered)
}

async fn contacts_edit_get(
    State(app_state): State<AppState>,
    flashes: IncomingFlashes,
    Path(contact_id): Path<String>,
) -> impl IntoResponse {
    let contact = app_state
        .contacts
        .find(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap()
        .unwrap();

    let rendered = Html(
        Layout {
            flashes: &flashes,
            content: EditContactContent {
                contact: &contact,
                errors: None,
            },
        }
        .to_string(),
    );
    (flashes, rendered)
}

async fn contacts_edit_post(
    State(app_state): State<AppState>,
    flashes: IncomingFlashes,
    flash: Flash,
    Path(contact_id): Path<String>,
    Form(form): Form<NewContactForm>,
) -> Response {
    let contact_id = ContactId::new(contact_id.parse().unwrap());
    let contact = form.build_contact(contact_id);

    match app_state.contacts.update(&contact).await.unwrap() {
        Ok(_) => (
            flash.success("Updated Contact!"),
            Redirect::to(&format!("/contacts/{}", contact_id.value())),
        )
            .into_response(),
        Err(errors) => {
            let rendered = Layout {
                flashes: &flashes,
                content: EditContactContent {
                    contact: &contact,
                    errors: Some(errors),
                },
            }
            .to_string();
            (flashes, Html(rendered)).into_response()
        }
    }
}

async fn contacts_delete_post(
    State(app_state): State<AppState>,
    flash: Flash,
    Path(contact_id): Path<String>,
) -> impl IntoResponse {
    app_state
        .contacts
        .delete(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap();
    (flash.success("Deleted Contact!"), Redirect::to("/contacts"))
}

markup::define! {
    ContactsContent(contacts: Vec<Contact>, q: Option<String>) {
        form ."tool-bar"[action="/contacts", method="get"] {
            label [for="search"] { "Search Term" }
            input #search[type="search", name="q", value=q];
            input [type="submit", value="Search"];
        }
        table {
            thead {
                tr {
                    th { "First" } th { "Last" } th { "Phone" } th { "Email" }
                }
            }
            tbody {
                @for contact in contacts {
                    tr {
                        td { @contact.first() }
                        td { @contact.last() }
                        td { @contact.phone() }
                        td { @contact.email() }
                        td {
                            a [href=format!("/contacts/{}/edit", contact.id().value())] { "Edit" }
                            @{" "}
                            a [href=format!("/contacts/{}", contact.id().value())] { "View" }
                        }
                    }
                }
            }
        }
        p {
            a [href="contacts/new"] { "Add Contact" }
        }
    }

    NewContactContent<'a>(contact: Option<&'a Contact>, errors: Option<ContactErrors>) {
        form [action="/contacts/new", method="post"] {
            @ContactFieldSet{ contact, errors }
        }

        p {
            a [href="/contacts"] { "Back" }
        }
    }

    ViewContactContent<'a>(contact: &'a Contact) {
        h1 { @{format!("{} {}", contact.first(), contact.last())} }

        div {
            div { @{ format!("Phone: {}", contact.phone()) } }
            div { @{ format!("Email: {}", contact.email()) } }
        }

        p {
            a [href=format!("/contacts/{}/edit", contact.id().value())] { "Edit" }
            @{" "}
            a [href="/contacts"] { "Back" }
        }
    }

    EditContactContent<'a>(contact: &'a Contact, errors: Option<ContactErrors>) {
        form [action=format!("/contacts/{}/edit", contact.id().value()), method="post"] {
            @ContactFieldSet{ contact: &Some(contact), errors }
        }

        form [action=format!("/contacts/{}/delete", contact.id().value()), method="POST"] {
            button { "Delete Contact" }
        }

        p {
            a [href="/contacts"] { "Back" }
        }
    }

    ContactFieldSet<'a>(contact: &'a Option<&'a Contact>, errors: &'a Option<ContactErrors>) {
        fieldset {
            legend { "Contact Values" }
            p {
                label [for="email"] { "Email" }
                input #email[name="email", type="email", placeholder="Email",
                    value=contact.map(|c| c.email())];
                @if let Some(errors) = &errors {
                    span .error { @errors.email }
                }
            }
            p {
                label [for="first_name"] { "First Name" }
                input #first_name[name="first_name", type="text", placeholder="First Name",
                    value=contact.map(|c| c.first())];
                @if let Some(errors) = &errors {
                    span .error { @errors.first }
                }
            }
            p {
                label [for="last_name"] { "Last Name" }
                input #last_name[name="last_name", type="text", placeholder="Last Name",
                    value=contact.map(|c| c.last())];
                @if let Some(errors) = &errors {
                    span .error { @errors.last }
                }
            }
            p {
                label [for="phone"] { "Phone" }
                input #phone[name="phone", type="text", placeholder="Phone",
                    value=contact.map(|c| c.phone())];
                @if let Some(errors) = &errors {
                    span .error { @errors.phone }
                }
            }
            button { "Save" }
        }
    }
}

markup::define! {
    Layout<'a, T: markup::Render>(flashes: &'a IncomingFlashes, content: T) {
        @markup::doctype()
        html {
            head {
                title { "Contact App" }
                link [rel="stylesheet", href="https://unpkg.com/missing.css@1.1.1"];
                link [rel="stylesheet", href="/static/site.css"];
            }
        }
        body {
            main {
                @for (_, message) in flashes.iter() {
                    div .flash { @message }
                }
                @content
            }
        }
    }
}
