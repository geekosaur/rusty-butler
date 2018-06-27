use irc::client::prelude::*;
use regex::Regex;
use reqwest;

pub fn handler(client: &IrcClient, msg: &Message, config: &Config) {
    // TODO Lots of needless checks every time. How to avoid?
    let access_token = get_access_token(config);
    if access_token.is_none() {
        return ()
    }
    if let Command::PRIVMSG(ref channel, ref message) = msg.command {
        let segment_reply = handle_segments(message);
        match segment_reply {
            Some(segment_id) => client.send_privmsg(&channel, &segment_id).unwrap(),
            _ => (),
        }
    }
}

pub fn get_access_token(config: &Config) -> Option<&String> {
    let options = &config.options;
    match options {
        Some(hm) => hm.get("strava_access_token"),
        None => None
    }
}

fn handle_segments(msg: &str) -> Option<String> {
    let segment_regex = Regex::new(r"https?://www\.strava\.com/segments/(\d+)").unwrap();
    for captures in segment_regex.captures_iter(msg) {
        println!("{}", captures.get(1).unwrap().as_str());
        let segment = Segment::fetch(captures.get(1).unwrap().as_str());
        return match segment {
            Ok(segment) => Some(segment.to_string()),
            Err(e) => {
                println!("{}", e);
                None
            }
        }
    }
    None
}

#[derive(Deserialize, Debug)]
struct Segment {
    name: String,
    activity_type: String,
    distance: f64,
    average_grade: f64,
    effort_count: u32,
    athlete_count: u32,
    city: String,
    // State can be null
    state: Option<String>,
    country: String,
}
impl Segment {
    fn fetch(id: &str) -> Result<Segment, reqwest::Error> {
        let strava_token = "";
        let url = format!("https://www.strava.com/api/v3/segments/{}?access_token={}", id, strava_token);
        let mut req = reqwest::get(&url)?;
        println!("{}", req.url());
        req.json()
    }
}
impl ToString for Segment {
    fn to_string(&self) -> String {
        let distance = (self.distance / 100.0).floor() / 10.0;
        let state = match self.state {
            Some(ref s) => s,
            None => "-",
        };
        format!("[STRAVA SEGMENT] \"{name}\", {activity_type} of {distance}km @ {grade}%. {effort_count} attempts by {athlete_count} athletes. Located in {city}, {state}, {country}.",
                name = self.name,
                activity_type = self.activity_type,
                distance = distance,
                grade = self.average_grade,
                effort_count = self.effort_count,
                athlete_count = self.athlete_count,
                city = self.city,
                state = state,
                country = self.country)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuff() {
        let s = handle_segments("https://www.strava.com/segments/13874540?filter=overall");
        //let s = handle_segments("https://www.strava.com/segments/8750847?filter=overall");
        //let s = handle_segments("https://www.strava.com/segments/12609639?filter=overall");
        //let s = handle_segments("https://www.strava.com/segments/14630434?filter=overall");

        match s {
            Some(segment_id) => println!("{}", segment_id),
            None => (),
        }
        panic!("Stop!");
    }
}
