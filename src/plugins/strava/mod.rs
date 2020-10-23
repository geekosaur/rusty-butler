use super::formatting;
use irc::client::prelude::*;
use regex::Regex;
use std::error;
use std::fmt;
use std::str::FromStr;
use unicode_segmentation::UnicodeSegmentation;

mod segment;
mod strava_irc_link;

// TODO: Get rid of things relying on access_token, I think strava's more privacy oriented setup
// ruins them and they are never used.

pub struct StravaHandler {
    access_token: Option<String>,
    segment_matcher: Regex,
    irc_links: strava_irc_link::StravaIrcLink,
}

impl StravaHandler {
    pub fn new(config: &Config) -> StravaHandler {
        let segment_matcher = Regex::new(r"https?://www\.strava\.com/segments/(\d+)").unwrap();
        let irc_links = strava_irc_link::StravaIrcLink::from_file_or_new("irc_links.json");
        match config.options.get("strava_access_token") {
            Some(access_token) => StravaHandler {
                access_token: Some(access_token.clone()),
                segment_matcher,
                irc_links,
            },
            None => {
                println!("No Strava access token, disabling plugin.");
                StravaHandler {
                    access_token: None,
                    segment_matcher,
                    irc_links,
                }
            }
        }
    }

    fn handle_segments(&self, msg: &str, access_token: &str) -> Option<String> {
        if let Some(captures) = self.segment_matcher.captures(msg) {
            println!("{}", captures.get(1).unwrap().as_str());
            let segment = segment::Segment::fetch(captures.get(1).unwrap().as_str(), access_token);
            return match segment {
                Ok(segment) => Some(segment.to_string()),
                Err(e) => {
                    println!("{}", e);
                    None
                }
            };
        }
        None
    }

    fn match_club(msg: &str) -> bool {
        let first_seven: String = msg.graphemes(true).take(7).collect();
        first_seven.eq_ignore_ascii_case("!strava")
    }

    fn handle_club(&self, msg: &str, access_token: &str) -> Vec<String> {
        let mut result = vec![];
        let input: String = msg.graphemes(true).skip(7).collect();
        let input = input.trim();
        println!("Handling club");
        let club_id = "freenode_running";
        let club = Club::fetch(club_id, access_token);
        let leaderboard = ClubLeaderboard::fetch(club_id, access_token);
        match club {
            Ok(club) => result.push(format!(
                "{club} https://www.strava.com/clubs/{club_id}",
                club = club,
                club_id = club_id
            )),
            Err(e) => eprintln!("Club::fetch failed: {}", e),
        }
        match leaderboard {
            Ok(mut leaderboard) => {
                match input.parse() {
                    Ok(sort_by) => leaderboard.sort(sort_by),
                    Err(e) => eprintln!(
                        "Failed to parse leaderboard sort, default sort used. Error: {}",
                        e
                    ),
                }
                // Note that this removes names not in the strava links file!!
                leaderboard.override_names(&self.irc_links);
                leaderboard.drop_ignored(&self.irc_links);
                result.push(leaderboard.to_string())
            }
            Err(e) => eprintln!("Error fetching leaderboard: {}", e),
        }
        result
    }
}

impl super::Handler for StravaHandler {
    fn handle(&self, client: &Client, msg: &Message) {
        if let Some(ref access_token) = self.access_token {
            if let Command::PRIVMSG(ref channel, ref message) = msg.command {
                let segment_reply = self.handle_segments(message, &access_token);
                if let Some(segment_id) = segment_reply {
                    client.send_privmsg(&channel, &segment_id).unwrap()
                }
                if StravaHandler::match_club(message) {
                    let club_reply = self.handle_club(message, &access_token);
                    for reply in club_reply {
                        println!("SEND: {}", reply);
                        client.send_privmsg(&channel, &reply).unwrap()
                    }
                }
            }
        }
    }
}

#[derive(Deserialize, Debug)]
struct Club {
    name: String,
    sport_type: String,
    member_count: u32,
}
impl Club {
    fn fetch(id: &str, access_token: &str) -> Result<Club, reqwest::Error> {
        let url = format!(
            "https://www.strava.com/api/v3/clubs/{}?access_token={}",
            id, access_token
        );
        let mut req = reqwest::get(&url)?;
        println!("{}", req.url());
        req.json()
    }
}
impl fmt::Display for Club {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[STRAVA CLUB] {name}, a {sport_type} club with {member_count} members.",
            name = self.name,
            sport_type = self.sport_type,
            member_count = self.member_count
        )
    }
}

#[derive(Deserialize, Debug)]
struct ClubLeaderboard {
    #[serde(rename = "data")]
    ranking: Vec<ClubLeaderboardAthlete>,
    // The following is never part of the json, but we want a default there anyway
    #[serde(default)]
    sorted_by: ClubLeaderboardSort,
}

impl ClubLeaderboard {
    fn fetch(id: &str, _access_token: &str) -> Result<ClubLeaderboard, reqwest::Error> {
        let url = format!("https://www.strava.com/clubs/{}/leaderboard", id);
        // More involved than the others because we need to change headers
        let client = reqwest::Client::new();
        let mut req = client.get(&url)
            .header("Accept", "text/javascript, application/javascript, application/ecmascript, application/x-ecmascript")
            .header("X-Requested-With", "XmlHttpRequest")
            .send()?;
        println!("{}", req.url());
        req.json()
    }
    fn sort(&mut self, sort_by: ClubLeaderboardSort) {
        if sort_by == self.sorted_by {
            return;
        }
        match sort_by {
            ClubLeaderboardSort::Distance => {
                self.ranking.sort_unstable_by_key(|a| -a.distance as i64)
            }
            ClubLeaderboardSort::Elevation => {
                self.ranking.sort_unstable_by_key(|a| -a.elev_gain as i64)
            }
            ClubLeaderboardSort::Moving => self
                .ranking
                .sort_unstable_by_key(|a| -i64::from(a.moving_time)),
            ClubLeaderboardSort::Pace => self
                .ranking
                .sort_unstable_by_key(|a| -(a.velocity * 1000.0) as i64),
            ClubLeaderboardSort::Slope => self
                .ranking
                .sort_unstable_by_key(|a| -(1000.0 * a.elev_gain / a.distance) as i64),
        }
    }
    fn override_names(&mut self, irc_links: &strava_irc_link::StravaIrcLink) {
        self.ranking.iter_mut().for_each(|athlete| {
            if let Some(nick) = irc_links.get_first_nick(athlete.strava_id) {
                athlete.first_name = nick.to_owned()
            }
        })
    }
    fn drop_ignored(&mut self, irc_links: &strava_irc_link::StravaIrcLink) {
        self.ranking
            .retain(|athlete| !irc_links.is_ignored(athlete.strava_id));
    }
}

impl fmt::Display for ClubLeaderboard {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let ranking = self
            .ranking
            .iter()
            .take(10)
            .enumerate()
            .map(|(idx, athlete)| format!("{idx}. {athlete}", idx = idx + 1, athlete = athlete,))
            .fold("".to_string(), |acc, ele| format!("{} {}", acc, ele));
        // Space too many at the start so we use it here instead
        write!(f, "🏆{ranking}", ranking = ranking)
    }
}

#[derive(Deserialize, Debug)]
struct ClubLeaderboardAthlete {
    #[serde(rename = "athlete_id")]
    strava_id: u64,
    #[serde(rename = "athlete_firstname")]
    first_name: String,
    distance: f64,
    moving_time: u32,
    elev_gain: f64,
    // Using for sorting (can I use it to get the pace/km number?)
    velocity: f64,
}

impl ClubLeaderboardAthlete {
    /// To prevent triggering people's highlights in IRC, add a zero width space after the first
    /// character. Possible problem: seems to screw up things at times in weechat used through
    /// iTerm2.
    fn prevent_irc_highlight(input: &str) -> String {
        let mut newname = input.to_owned();
        let mut idx = 1;
        while !input.is_char_boundary(idx) {
            idx += 1;
        }
        newname.insert(idx, '\u{200d}');
        newname
    }
}

impl fmt::Display for ClubLeaderboardAthlete {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let distance = (self.distance / 1000.0).floor();
        let pace = (f64::from(self.moving_time) / (self.distance / 1000.0)).round() as u32;
        let elev_gain = self.elev_gain.round() as u32;
        // Percentage
        let slope = self.elev_gain / self.distance * 100.0;
        let slope = format!("{:.1}%", slope);
        // Moving time format
        let hours = (f64::from(self.moving_time) / 3600.0) as u32;
        let minutes = ((f64::from(self.moving_time) % 3600.0) / 60.0) as u32;
        let moving_time = format!("{}h{:02}", hours, minutes);
        // TODO: It was already long, but adding slope makes it way too long
        write!(
            f,
            "{format_start}{first_name}{format_end} {distance}k {moving_time} {pace}/k ↑{elev_gain}m {slope}",
            first_name = ClubLeaderboardAthlete::prevent_irc_highlight(&self.first_name),
            distance = distance,
            moving_time = moving_time,
            pace = format_time(pace),
            elev_gain = elev_gain,
            slope = slope,
            format_start = formatting::IrcFormat::Bold,
            format_end = formatting::IrcFormat::Normal,
        )
    }
}

/// Enum to handle the different inputs by which the leaderboard can be sorted.
/// Ensures in the actual sorting we only deal with some known values. The input string is parsed
/// into one of the enum's values.
#[derive(Debug, Deserialize, PartialEq)]
enum ClubLeaderboardSort {
    Elevation,
    Distance,
    Moving,
    Pace,
    Slope,
}

impl Default for ClubLeaderboardSort {
    fn default() -> Self {
        ClubLeaderboardSort::Distance
    }
}

impl FromStr for ClubLeaderboardSort {
    type Err = ParseClubLeaderboardSortError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "elev" | "elevation" | "vertical" | "climb" | "climbing" => {
                Ok(ClubLeaderboardSort::Elevation)
            }
            "distance" | "dist" | "length" | "len" => Ok(ClubLeaderboardSort::Distance),
            "moving" | "time" | "duration" => Ok(ClubLeaderboardSort::Moving),
            "pace" | "speed" | "velocity" => Ok(ClubLeaderboardSort::Pace),
            "slope" | "steep" | "steepness" => Ok(ClubLeaderboardSort::Slope),
            _ => Err(ParseClubLeaderboardSortError),
        }
    }
}

#[derive(Debug, Clone)]
struct ParseClubLeaderboardSortError;

impl error::Error for ParseClubLeaderboardSortError {
    fn description(&self) -> &str {
        "Failed to parse sorting parameter."
    }
    fn cause(&self) -> Option<&dyn error::Error> {
        None
    }
}

impl fmt::Display for ParseClubLeaderboardSortError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Failed to parse sorting parameter.")
    }
}

/// Formats a given amount of seconds to the form m:ss or h:mm:ss, depending on the length.
fn format_time(seconds: u32) -> String {
    let hours = (f64::from(seconds) / 3600.0).floor();
    let minutes = (f64::from(seconds % 3600) / 60.0).floor();
    let seconds = seconds % 60;
    if hours == 0.0 {
        return format!("{}:{:02}", minutes, seconds);
    } else {
        return format!("{}:{:02}:{:02}", hours, minutes, seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_club() {
        let input = "!strava";
        assert!(StravaHandler::match_club(input));
        let input = "!stravasdifohoefsbv";
        assert!(StravaHandler::match_club(input));
        let input = "!strava pace";
        assert!(StravaHandler::match_club(input));
    }

    #[test]
    fn match_club_and_unicode() {
        // Input starts with some unicode.
        // In production we got a panic that we were splitting halfway through a character.
        // This crashed the bot
        let input = "🏃🏃";
        assert!(!StravaHandler::match_club(input));
    }

    #[test]
    fn irc_highlight_prevention() {
        assert_eq!(
            ClubLeaderboardAthlete::prevent_irc_highlight("ward"),
            "w‍ard"
        );
        assert_eq!(
            ClubLeaderboardAthlete::prevent_irc_highlight("Žilvinas"),
            "Ž‍ilvinas"
        );
        assert_eq!(
            ClubLeaderboardAthlete::prevent_irc_highlight("🇧🇪🇧🇪🇧🇪"),
            "🇧‍🇪🇧🇪🇧🇪"
        );
    }

    #[test]
    fn athlete_display() {
        let athlete = ClubLeaderboardAthlete {
            strava_id: 12345,
            first_name: "ward".to_owned(),
            distance: 10_000.0,
            moving_time: 60 * 50,
            elev_gain: 100.0,
            velocity: 20.0,
        };
        assert_eq!(
            athlete.to_string(),
            "\u{2}w\u{200d}ard\u{f} 10k 0h50 5:00/k ↑100m 1.0%"
        );
        let leaderboard = ClubLeaderboard {
            ranking: vec![athlete],
            sorted_by: ClubLeaderboardSort::Slope,
        };
        assert_eq!(
            leaderboard.to_string(),
            "🏆 1. \u{2}w\u{200d}ard\u{f} 10k 0h50 5:00/k ↑100m 1.0%"
        );
    }
}
