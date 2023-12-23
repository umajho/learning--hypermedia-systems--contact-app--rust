mod contact_model;
mod contact_repo;
mod contacts_archiver;
mod laying_out;
mod static_assets;

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{FromRef, Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::{AppendHeaders, Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Extension, Router,
};
use axum_extra::extract::Form;
use axum_flash::{Flash, IncomingFlashes};
use axum_htmx::{HxRequest, HxTrigger};
use contacts_archiver::Archiver;
use laying_out::Layouter;
use serde::Deserialize;
use sqlx::sqlite::SqlitePoolOptions;
use static_assets::StaticFile;
use tower_http::catch_panic::CatchPanicLayer;

use contact_model::{Contact, ContactErrors, ContactId};
use contact_repo::ContactRepo;

#[derive(Clone)]
struct AppState {
    /// FIXME: not working for Safari (on localhost?).
    flash_config: axum_flash::Config,

    contacts: Arc<ContactRepo>,
    archiver: Arc<Archiver>,
}
impl FromRef<AppState> for axum_flash::Config {
    fn from_ref(state: &AppState) -> Self {
        state.flash_config.clone()
    }
}

const FAKE_CONTACTS: u32 = 100;

#[tokio::main]
async fn main() {
    let pool = SqlitePoolOptions::new()
        .max_lifetime(None)
        .idle_timeout(None)
        .connect(":memory:")
        .await
        .unwrap();

    let flash_config = axum_flash::Config::new(axum_flash::Key::generate());
    let contacts = Arc::new(
        ContactRepo::build_with_fake_data(pool, FAKE_CONTACTS)
            .await
            .unwrap(),
    );
    let archiver = Arc::new(Archiver::new(contacts.clone()));
    let app_state = AppState {
        flash_config,
        contacts,
        archiver,
    };

    let app = Router::new()
        .route("/static/*path", get(static_assets_get))
        .route("/", get(root))
        .route("/contacts", get(contacts_get))
        .route("/contacts/archive", post(contacts_archive_post))
        .route("/contacts/archive", get(contacts_archive_get))
        .route("/contacts/archive", delete(contacts_archive_delete))
        .route("/contacts/archive/file", get(contacts_archive_file_get))
        .route("/contacts/count", get(contacts_count_get))
        .route("/contacts/new", get(contacts_new_get))
        .route("/contacts/new", post(contacts_new_post))
        .route("/contacts/:contact_id", get(contacts_view_get))
        .route("/contacts/:contact_id/edit", get(contacts_edit_get))
        .route("/contacts/:contact_id/edit", post(contacts_edit_post))
        .route("/contacts/:contact_id/delete", post(contacts_delete_post))
        .route("/contacts/:contact_id", delete(contacts_delete_post))
        .route("/contacts", delete(contacts_delete))
        .route("/contacts/validate-email", get(contacts_validate_email))
        .layer(middleware::from_fn(laying_out::with_layouter))
        .layer(CatchPanicLayer::new())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn static_assets_get(Path(path): Path<String>) -> impl IntoResponse {
    StaticFile(path)
}

async fn root() -> impl IntoResponse {
    Redirect::to("/contacts")
}

#[derive(Deserialize)]
struct ContactsQuery {
    q: Option<String>,
    page: Option<u32>,
}

async fn contacts_get(
    State(app_state): State<AppState>,
    Extension(Layouter(layouter)): Extension<Layouter>,
    HxTrigger(htmx_trigger): HxTrigger,
    flashes: IncomingFlashes,
    Query(query): Query<ContactsQuery>,
) -> impl IntoResponse {
    let q = query.q.map(|q| q.trim().to_string());
    let page = query.page.unwrap_or(1);
    let contacts_set = match &q {
        Some(q) if !q.is_empty() => app_state.contacts.search(q, page).await,
        _ => app_state.contacts.all_by_page(page).await,
    }
    .unwrap();

    let rendered = if htmx_trigger.as_deref() == Some("search") {
        Html(
            ContactsTableRows {
                contacts: &contacts_set,
                q: &q.as_deref(),
                page: &page,
            }
            .to_string(),
        )
    } else {
        let content = ContactsContent {
            contacts: contacts_set,
            q: q.as_deref(),
            page,
            archiver: &app_state.archiver,
        };
        layouter(flashes.clone(), markup::new!(@content))
    };

    (flashes, rendered)
}

async fn contacts_archive_post(State(app_state): State<AppState>) -> impl IntoResponse {
    app_state.archiver.run();
    Html(
        (ArchiveUi {
            archiver: &app_state.archiver,
        })
        .to_string(),
    )
}

async fn contacts_archive_get(State(app_state): State<AppState>) -> impl IntoResponse {
    Html(
        (ArchiveUi {
            archiver: &app_state.archiver,
        })
        .to_string(),
    )
}

async fn contacts_archive_delete(State(app_state): State<AppState>) -> impl IntoResponse {
    app_state.archiver.reset();

    Html(
        (ArchiveUi {
            archiver: &app_state.archiver,
        })
        .to_string(),
    )
}

async fn contacts_archive_file_get(State(app_state): State<AppState>) -> Response {
    let headers = AppendHeaders([
        (header::CONTENT_TYPE, "application/json; charset=utf8"),
        (
            header::CONTENT_DISPOSITION,
            r#"attachment; filename="archive.json""#,
        ),
    ]);

    let Some(json_data) = app_state.archiver.json_data() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let json_data = json_data.to_string();

    (headers, json_data).into_response()
}

async fn contacts_count_get(State(app_state): State<AppState>) -> impl IntoResponse {
    let count = app_state.contacts.count().await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    Html(html_escape::encode_text(&format!("({} total Contacts)", count)).to_string())
}

async fn contacts_new_get(
    Extension(Layouter(layouter)): Extension<Layouter>,
    flashes: IncomingFlashes,
) -> impl IntoResponse {
    let content = NewContactContent {
        contact: None,
        errors: None,
    };
    let rendered = layouter(flashes.clone(), markup::new!(@content));
    (flashes, rendered)
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
    Extension(Layouter(layouter)): Extension<Layouter>,
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
            let content = NewContactContent {
                contact: Some(&contact),
                errors: Some(errors),
            };
            let rendered = layouter(flashes.clone(), markup::new!(@content));
            (flashes, rendered).into_response()
        }
    }
}

async fn contacts_view_get(
    State(app_state): State<AppState>,
    Extension(Layouter(layouter)): Extension<Layouter>,
    flashes: IncomingFlashes,
    Path(contact_id): Path<String>,
) -> impl IntoResponse {
    let contact = app_state
        .contacts
        .find(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap()
        .unwrap();

    let content = ViewContactContent { contact: &contact };
    let rendered = layouter(flashes.clone(), markup::new!(@content));
    (flashes, rendered)
}

async fn contacts_edit_get(
    State(app_state): State<AppState>,
    Extension(Layouter(layouter)): Extension<Layouter>,
    flashes: IncomingFlashes,
    Path(contact_id): Path<String>,
) -> impl IntoResponse {
    let contact = app_state
        .contacts
        .find(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap()
        .unwrap();

    let content = EditContactContent {
        contact: &contact,
        errors: None,
    };
    let rendered = layouter(flashes.clone(), markup::new!(@content));
    (flashes, rendered)
}

async fn contacts_edit_post(
    State(app_state): State<AppState>,
    Extension(Layouter(layouter)): Extension<Layouter>,
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
            let content = EditContactContent {
                contact: &contact,
                errors: Some(errors),
            };
            let rendered = layouter(flashes.clone(), markup::new!(@content));
            (flashes, rendered).into_response()
        }
    }
}

async fn contacts_delete_post(
    State(app_state): State<AppState>,
    HxRequest(is_htmx_request): HxRequest,
    HxTrigger(htmx_trigger): HxTrigger,
    flash: Flash,
    Path(contact_id): Path<String>,
) -> Response {
    app_state
        .contacts
        .delete(ContactId::new(contact_id.parse().unwrap()))
        .await
        .unwrap();
    if !is_htmx_request || htmx_trigger.as_deref() == Some("delete-btn") {
        (flash.success("Deleted Contact!"), Redirect::to("/contacts")).into_response()
    } else {
        Html("").into_response()
    }
}

#[derive(Deserialize)]
struct DeleteContactsForm {
    selected_contact_ids: Vec<u32>,
}

/// NOTE: In Chapter 06, the book let this function response the content as if
/// were requested by a “GET” verb, instead of redirecting like how it is done
/// here. the book also removed `hx-push-url` from the button that can issue
/// this route. The way the book handled ignored URL queries, and I'm too lazy
/// to take care of them, but I also don't want to make them broken as the book
/// did, so I chose the simple way to just redirect the page to the URL without
/// queries.
async fn contacts_delete(
    State(app_state): State<AppState>,
    flash: Flash,
    Form(form): Form<DeleteContactsForm>,
) -> impl IntoResponse {
    for contact_id in form.selected_contact_ids {
        app_state
            .contacts
            .delete(ContactId::new(contact_id))
            .await
            .unwrap();
    }

    (
        flash.success("Deleted Contacts!"),
        Redirect::to("/contacts"),
    )
}

#[derive(Deserialize)]
struct ValidateContactEmailForm {
    email: String,
    contact_id: Option<u32>,
}

async fn contacts_validate_email(
    State(app_state): State<AppState>,
    Form(form): Form<ValidateContactEmailForm>,
) -> impl IntoResponse {
    let error_text = app_state
        .contacts
        .validate_email(form.contact_id.map(ContactId::new), form.email)
        .await
        .unwrap()
        .unwrap_or("".to_string());
    Html(html_escape::encode_text(&error_text).to_string())
}

markup::define! {
    ContactsContent<'a>(contacts: Vec<Contact>, q: Option<&'a str>, page: u32, archiver: &'a Archiver) {
        // div {
        //     span [style="float: right"] {
        //         @if *page > 1 {
        //             a [
        //                 "hx-get"=format!("/contacts?page={}", page-1),
        //                 "hx-target"="body",
        //                 "hx-swap"="outerHTML",
        //                 "hx-push-url"="true",
        //                 "hx-vals"=q.map(|q| serde_json::json!({ "q": q }).to_string()),
        //             ] { "Previous" }
        //         }
        //         @{" "}
        //         @if contacts.len() == (contact_repo::PAGE_SIZE as usize) {
        //             a [
        //                 "hx-get"=format!("/contacts?page={}", page+1),
        //                 "hx-target"="body",
        //                 "hx-swap"="outerHTML",
        //                 "hx-push-url"="true",
        //                 "hx-vals"=q.map(|q| serde_json::json!({ "q": q }).to_string()),
        //             ] { "Next" }
        //         }
        //     }
        // }

        @ArchiveUi{ archiver }
        form ."tool-bar"[action="/contacts", method="get"] {
            label [for="search"] { "Search Term" }
            input #search[
                type="search", name="q", value=q,
                "hx-get"="/contacts",
                "hx-trigger"="search, change, keyup delay:200ms changed",
                "hx-target"="tbody",
                "hx-push-url"="true",
                "hx-indicator"="#spinner",
            ];
            img #spinner."htmx-indicator"[
                style="height: 20px",
                src="/static/img/spinning-circles.svg",
                alt="Request In Flight...",
            ];
            input [type="submit", value="Search"];
        }
        p {
            a [href="contacts/new"] { "Add Contact" }
            @{" "}
            span ["hx-get"="/contacts/count", "hx-trigger"="revealed"/*"load"*/] {
                img ."htmx-indicator"[
                    style="height: 20px",
                    src="/static/img/spinning-circles.svg",
                ];
            }
        }
        form {
            button [
                "hx-delete"="/contacts",
                "hx-push-url"="true", // NOTE: See [`contacts_delete`].
                "hx-confirm"="Are you sure you want to delete these contacts?",
                "hx-target"="#content",
            ] { "Delete Selected Contacts" }
            table {
                thead {
                    tr {
                        th; th { "First" } th { "Last" } th { "Phone" } th { "Email" }
                    }
                }
                tbody {
                    @ContactsTableRows { contacts, q, page }
                }
            }
        }
    }

    ArchiveUi<'a>(archiver: &'a Archiver) {
        div #"archive-ui"["hx-target"="this", "hx-swap"="outerHTML"] {
            @match archiver.status() {
                contacts_archiver::Status::Waiting => {
                    button ["hx-post"="/contacts/archive"] {
                        "Download Contact Archive"
                    }
                }
                contacts_archiver::Status::Running => {
                    div ["hx-get"="/contacts/archive", "hx-trigger"="load delay:500ms"] {
                        "Creating Archive…"
                        div .progress {
                            div #"archive-progress"."progress-bar"[
                                role="progressbar",
                                "aria-valuenow"={archiver.progress() * 100.0},
                                "style"=format!("width: {}%", archiver.progress() * 100.0),
                            ];
                        }
                    }
                }
                contacts_archiver::Status::Complete => {
                    a ["hx-boost"="false", href="/contacts/archive/file"] {
                        "Archive Ready! Click here to download. ↓"
                    }
                    @{" "}
                    button ["hx-delete"="/contacts/archive"] {
                        "Clear Download"
                    }
                }
            }
        }
    }

    ContactsTableRows<'a>(contacts: &'a Vec<Contact>, q: &'a Option<&'a str>, page: &'a u32) {
        @for contact in contacts.iter() {
            tr {
                td {
                    input [type="checkbox", name="selected_contact_ids", value=contact.id().value()];
                }
                td { @contact.first() }
                td { @contact.last() }
                td { @contact.phone() }
                td { @contact.email() }
                td {
                    a [href=format!("/contacts/{}/edit", contact.id().value())] { "Edit" }
                    @{" "}
                    a [href=format!("/contacts/{}", contact.id().value())] { "View" }
                    @{" "}
                    a [
                        "hx-delete"=format!("/contacts/{}", contact.id().value()),
                        "hx-swap"="outerHTML swap:1s",
                        "hx-confirm"="Are you sure you want to delete this contact?",
                        "hx-target"="closest tr",
                    ] {
                        "Delete"
                    }
                }
            }
        }
        @if contacts.len() == 10 {
            tr {
                td [colspan="5", style="text-align: center"] {
                    // botton [
                    //     "hx-target"="closest tr",
                    //     "hx-swap"="outerHTML",
                    //     "hx-select"="tbody > tr",
                    //     "hx-get"=format!("/contacts?page={}", page + 1),
                    //     "hx-vals"=q.map(|q| serde_json::json!({ "q": q }).to_string()),
                    // ] { "Load More" }
                    span [
                        "hx-target"="closest tr",
                        "hx-trigger"="revealed",
                        "hx-swap"="outerHTML",
                        "hx-select"="tbody > tr",
                        "hx-get"=format!("/contacts?page={}", *page + 1),
                        "hx-vals"=q.map(|q| serde_json::json!({ "q": q }).to_string()),
                    ] { "Loading More…" }
                }
            }
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
            button #"delete-btn"[
                "hx-delete"=format!("/contacts/{}", contact.id().value()),
                "hx-push-url"="true",
                "hx-confirm"="Are you sure you want to delete this contact?",
                "hx-target"="#content",
            ] {
                "Delete Contact"
            }
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
                input #email[
                    name="email", type="email", placeholder="Email",
                    value=contact.map(|c| c.email()),
                    "hx-get"="/contacts/validate-email",
                    "hx-target"="next .error",
                    "hx-trigger"="change, keyup delay:200ms changed",
                    "hx-vals"=contact.map(|c| serde_json::json!({
                        "contact_id": c.id().value()
                    }).to_string()),
                ];
                span .error {
                    @errors.as_ref().and_then(|errs| errs.email.as_deref())
                }
            }
            p {
                label [for="first_name"] { "First Name" }
                input #first_name[name="first_name", type="text", placeholder="First Name",
                    value=contact.map(|c| c.first())];
                span .error {
                    @errors.as_ref().and_then(|errs| errs.first.as_deref())
                }
            }
            p {
                label [for="last_name"] { "Last Name" }
                input #last_name[name="last_name", type="text", placeholder="Last Name",
                    value=contact.map(|c| c.last())];
                span .error {
                    @errors.as_ref().and_then(|errs| errs.last.as_deref())
                }
            }
            p {
                label [for="phone"] { "Phone" }
                input #phone[name="phone", type="text", placeholder="Phone",
                    value=contact.map(|c| c.phone())];
                span .error {
                    @errors.as_ref().and_then(|errs| errs.phone.as_deref())
                }
            }
            button { "Save" }
        }
    }
}
