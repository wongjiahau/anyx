//! The generators read a schema of bearer and public endpoints.

const SCHEMA: &str = include_str!("fixtures/endpoints.anyx");

#[test]
fn splits_the_served_handler_by_endpoint() {
    let schema = anyx_core_generate::load(SCHEMA, "endpoints.anyx").unwrap();
    let rust = anyx_core_generate::rust(&schema).unwrap();
    // A public endpoint's handler gets no account; a bearer one gets one.
    assert!(rust.contains(
        "async fn sign_in_password(&self, email: String, password: String) -> Reply<Session, SignInError>;"
    ));
    assert!(rust.contains(
        "async fn list_files(&self, account: Self::Account) -> Reply<Vec<FileSummary>, Never>;"
    ));
    // The server refuses a credential that doesn't fit the endpoint.
    assert!(rust.contains("`credential` doesn't fit `list files`"));
}

#[test]
fn a_client_method_takes_the_credential_its_endpoint_fits() {
    let schema = anyx_core_generate::load(SCHEMA, "endpoints.anyx").unwrap();
    let typescript = anyx_core_generate::typescript(&schema).unwrap();
    assert!(typescript.contains("listFiles(token: string): Promise<Reply<FileSummary[], never>>"));
    assert!(typescript.contains(
        "signInPassword(email: string, password: string): Promise<Reply<Session, SignInError>>"
    ));
}
