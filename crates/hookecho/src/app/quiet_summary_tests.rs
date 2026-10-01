use super::quiet_summary;

fn held(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("alert {i}"), "body".to_string()))
        .collect()
}

#[test]
fn one_alert_reads_singular() {
    let (title, body) = quiet_summary(&held(1));
    assert_eq!(title, "1 alert while you were away");
    assert_eq!(body, "alert 0");
}

#[test]
fn many_alerts_name_a_few_and_count_the_rest() {
    let (title, body) = quiet_summary(&held(9));
    assert_eq!(title, "9 alerts while you were away");
    assert!(body.starts_with("alert 0\nalert 1\nalert 2\nalert 3\n"));
    assert!(body.ends_with("(+5 more)"));
}

#[test]
fn exactly_the_named_count_has_no_tail() {
    let (_, body) = quiet_summary(&held(4));
    assert!(!body.contains("more"));
}
