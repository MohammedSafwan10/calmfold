//! Fills a database with sample notes, tasks and events for manual testing and screenshots.
//!
//!     cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3
//!     KEEPSTONE_DATA_DIR=/tmp/ks cargo run
//!
//! Never point this at a real data folder.
#![allow(clippy::expect_used)]

use keepstone_core::{Event, Note, Priority, Recurrence, Task, TaskStatus, clock, parse_event};
use keepstone_storage::Store;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: seed <path/to/keepstone.sqlite3>");
    let mut store = Store::open(path.as_ref()).expect("open database");
    let now = clock::now();
    let today = clock::today();
    let day = |offset: i64| {
        today
            .checked_add(jiff::Span::new().days(offset))
            .expect("date")
    };

    let mut car = Note::new(
        "Car",
        "# Car\nInsurance renews in **May**. See [[Service log]] for details.\n\n- Plate: KA-01\n- Oil: `5W-30`\n",
        now,
    )
    .expect("note");
    car.set_pinned(true, now).expect("pin");
    for note in [
        car,
        Note::new(
            "Service log",
            "Oil changed at 42,000 km. Next service at 52,000 km.",
            now,
        )
        .expect("note"),
        Note::new(
            "Reading list",
            "- The Pragmatic Programmer\n- Designing Data-Intensive Applications",
            now,
        )
        .expect("note"),
        Note::new(
            "Trip ideas",
            "Coorg in December. Check homestays, see [[Car]] for the service.",
            now,
        )
        .expect("note"),
    ] {
        store.save_note(&note).expect("save note");
    }

    let tasks = [
        ("Pay rent", Some(day(0)), Priority::High, TaskStatus::Todo),
        (
            "Renew car insurance",
            Some(day(-2)),
            Priority::Medium,
            TaskStatus::InProgress,
        ),
        ("Call mom", Some(day(0)), Priority::None, TaskStatus::Done),
        (
            "Book dentist",
            Some(day(5)),
            Priority::None,
            TaskStatus::Todo,
        ),
        ("Read 20 pages", None, Priority::Low, TaskStatus::Todo),
    ];
    for (title, due, priority, status) in tasks {
        let mut task = Task::new(title, now).expect("task");
        task.set_due(due, now).expect("due");
        task.set_priority(priority, now).expect("priority");
        task.transition(status, now).expect("status");
        store.save_task(&task).expect("save task");
    }

    // Events, written the way a user types them in quick add.
    for (text, offset) in [
        ("Standup 9:30-9:45 every weekday", 0),
        ("Design review 2pm-3:30pm", 1),
        ("Lunch with Sam 1pm", 2),
        ("Dentist 4pm for 45m", 5),
        ("Gym 7am", 0),
        ("Focus block 10am-12pm", 1),
        ("Call with Priya 10:30am", 1),
        ("Trip to Coorg", 12),
        ("Mom's birthday yearly", 9),
        ("Flight 10pm-2am", 6),
    ] {
        let draft = parse_event(text, today).on_default_day(day(offset));
        let mut event = if draft.all_day {
            Event::new_all_day(&draft.title, draft.start.date(), draft.last_day(), now)
        } else {
            Event::new_timed(&draft.title, draft.start, draft.end, now)
        }
        .expect("event");
        if draft.title == "Trip to Coorg" {
            event
                .set_all_day_dates(day(12), day(14), now)
                .expect("trip");
            event.set_location("Madikeri", now).expect("location");
        }
        event.set_recurrence(draft.recurrence, now).expect("repeat");
        if draft.title == "Gym" {
            event
                .set_recurrence(Some(Recurrence::DAILY), now)
                .expect("repeat");
        }
        store.save_event(&event).expect("save event");
    }
    println!("Seeded {path}");
}
