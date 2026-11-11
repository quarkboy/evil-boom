//! The follower must notice it's off the route (warped into a side room 6 s in, as if blasted
//! there), plan again from where it is, and still exit. (A process of its own: the game is one
//! a process.)

#[path = "common/drive.rs"]
mod drive;

#[test]
fn bot_recovers_after_being_knocked_off_course() {
    let Some((exited, replans, events)) = drive::drive(true) else {
        return;
    };
    assert!(exited, "{}", events.join("\n"));
    assert!(replans >= 1, "{}", events.join("\n"));
}
