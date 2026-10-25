//! How a `/library/sections` answer becomes the app's library kinds (`project_sections`): a movie
//! library filed with no metadata agent and the Plex Video Files scanner is Plex's Other Videos, and
//! everything ambiguous stays Movies.

use super::*;

/// The sections of a real server (PMS 1.43.5, measured; titles and ids are placeholders), in the
/// shape of the plain `/library/sections` answer: only `type`, `agent` and `scanner` tell the third
/// from a Movies library.
const THREE_SECTIONS: &str = r#"{"MediaContainer":{"size":3,"Directory":[
  {"key":"1","type":"movie","title":"Movies","agent":"tv.plex.agents.movie","scanner":"Plex Movie","hidden":0},
  {"key":"2","type":"show","title":"TV Shows","agent":"tv.plex.agents.series","scanner":"Plex TV Series","hidden":0},
  {"key":"3","type":"movie","title":"Other Videos","agent":"tv.plex.agents.none","scanner":"Plex Video Files","hidden":1}]}}"#;

fn project(json: &str) -> Vec<(i64, String, SecKind)> {
    let mc = serde_json::from_str::<plx_plex::plex::Envelope>(json).expect("parses").media_container;
    project_sections(&mc)
}

fn one_movie_type_section(agent: &str, scanner: &str) -> SecKind {
    let json = format!(r#"{{"MediaContainer":{{"Directory":[{{"key":"7","type":"movie","title":"Library","agent":"{agent}","scanner":"{scanner}"}}]}}}}"#);
    project(&json).remove(0).2
}

/// **The deliverable of #599.** The Other Videos library is `type: movie` on the wire, so it was
/// projected as a Movies library and amalgamated into the Movies pill.
#[test]
fn a_three_section_answer_projects_movies_shows_and_other_videos() {
    assert_eq!(
        project(THREE_SECTIONS),
        vec![
            (1, "Movies".to_string(), SecKind::Movie),
            (2, "TV Shows".to_string(), SecKind::Show),
            (3, "Other Videos".to_string(), SecKind::Video),
        ],
    );
}

/// A curated Movies library keeps the Plex Movie scanner whatever its agent is, and a hand-built one
/// with the Movie agent and the Video Files scanner is Movies too: a false positive would move a
/// library off the Movies pill, a false negative is how the app has always read Other Videos.
#[test]
fn a_movie_library_with_the_none_agent_but_the_movie_scanner_stays_movies() {
    assert_eq!(one_movie_type_section("tv.plex.agents.none", "Plex Movie"), SecKind::Movie);
    assert_eq!(one_movie_type_section("tv.plex.agents.movie", "Plex Video Files"), SecKind::Movie);
}

/// An older server, or a shared token's answer, may send neither field: every `movie` section is a
/// Movies library, as before.
#[test]
fn an_answer_without_agent_stays_movies() {
    let json = r#"{"MediaContainer":{"Directory":[
      {"key":"1","type":"movie","title":"Movies"},
      {"key":"2","type":"show","title":"TV Shows"},
      {"key":"3","type":"movie","title":"Clips","scanner":"Plex Video Files"}]}}"#;
    assert_eq!(
        project(json).into_iter().map(|(_, _, kind)| kind).collect::<Vec<_>>(),
        [SecKind::Movie, SecKind::Show, SecKind::Movie],
    );
}
