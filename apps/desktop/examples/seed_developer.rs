//! Disposable Developer/vault fixture for visual review. Refuses existing files.
//! cargo run -p ottero-desktop --example seed_developer -- /tmp/ottero-review/ottero.sqlite3
//! OTTERO_DATA_DIR=/tmp/ottero-review OTTERO_DEV_ROUTE=developer cargo run
//! Preview vault password is `ottero-preview-only`; contains no real secrets.
#![allow(clippy::expect_used)]
use ottero_core::developer::{DeveloperProject, ProjectLink};
use ottero_core::vault::environment::EnvironmentSecrets;
use ottero_core::vault::{ItemFields, ItemKind};
use ottero_core::{Note, Project, Task, clock};
use ottero_crypto::{KdfParams, SecretString};
use ottero_desktop::vault::{KdfInput, VaultSession, derive};
use ottero_storage::Store;
fn main() {
    let path = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("provide a new database path"),
    );
    assert!(!path.exists(), "Refusing to change an existing database");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    let mut store = Store::open(&path).expect("store");
    let now = clock::now();
    let mut first = None;
    for name in ["Ottero", "Personal website", "Home server"] {
        let mut project = DeveloperProject::new(Project::new(name, now).expect("project"));
        let note = Note::new(&format!("{name} — Setup"), "", now).expect("note");
        project.set_setup_note(note.id(), now);
        project
            .update(
                vec![
                    ProjectLink::new("Repository", "https://github.com/MohammedSafwan10/ottero")
                        .expect("link"),
                    ProjectLink::new("Website", "https://nexdark.com").expect("link"),
                ],
                "/tmp/ottero-preview",
                now,
            )
            .expect("details");
        store
            .save_developer_project(&project, Some(&note))
            .expect("save");
        let mut task = Task::new("Review the next release", now).expect("task");
        task.set_project(Some(project.id()), now).expect("project");
        store.save_task(&task).expect("save task");
        first.get_or_insert(project.id());
    }
    let mut vault = VaultSession::load(&store).expect("vault");
    let input = KdfInput::fresh(KdfParams::RECOMMENDED).expect("input");
    let key = derive(
        &SecretString::from("ottero-preview-only".to_owned()),
        &input.salt,
        input.params,
    )
    .expect("key");
    vault
        .finish_setup(&mut store, input, &key, None, now)
        .expect("setup");
    let parsed = EnvironmentSecrets::parse("DATABASE_URL=postgres://localhost/ottero\nAPI_TOKEN=demo-only\nAPP_URL=http://localhost:3000\nLOG_LEVEL=info\nPRIVATE_KEY='demo line one\ndemo line two'\n").expect("parse");
    let environment = EnvironmentSecrets::new(first, "Development", parsed.variables().to_vec())
        .expect("environment");
    let fields = ItemFields {
        title: "Ottero · Development".into(),
        environment: Some(environment),
        ..ItemFields::new(ItemKind::Note)
    };
    vault.create(&mut store, &fields, now).expect("item");
}
