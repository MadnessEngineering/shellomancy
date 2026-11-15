use super::*;

#[test]
fn a_refresh_keeps_the_last_data_on_screen() {
    let mut fetch = Fetch::Loaded { data: vec![1, 2] };

    fetch.start();

    assert!(fetch.is_loading());
    assert_eq!(fetch.data(), Some(&vec![1, 2]));
}

#[test]
fn a_failed_refresh_keeps_the_last_data_and_says_why() {
    let mut fetch = Fetch::Loaded { data: vec![1] };
    fetch.start();

    fetch.finish(Err("HTTP 502".to_owned()));

    assert_eq!(fetch.error(), Some("HTTP 502"));
    assert_eq!(fetch.data(), Some(&vec![1]));
}

#[test]
fn a_successful_refresh_replaces_the_data() {
    let mut fetch = Fetch::Failed {
        message: "HTTP 502".to_owned(),
        stale: Some(vec![1]),
    };
    fetch.start();

    fetch.finish(Ok(vec![3]));

    assert_eq!(fetch, Fetch::Loaded { data: vec![3] });
}

#[test]
fn a_first_fetch_has_nothing_to_show_yet() {
    let mut fetch: Fetch<Vec<u8>> = Fetch::Idle;

    fetch.start();

    assert_eq!(fetch.data(), None);
}
