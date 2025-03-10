use anyhow::anyhow;
use atrium_api::app::bsky::feed::post::{RecordData, ReplyRefData};
use atrium_api::app::bsky::richtext::facet::MainFeaturesItem::Mention;
use atrium_api::com::atproto::repo::strong_ref;
use atrium_api::types::Union;
use atrium_api::types::string::Datetime;
use atrium_api::record::KnownRecord::AppBskyFeedPost;
use bsky_sdk::BskyAgent;
use bsky_sdk::rich_text::RichText;
use clap::{Parser, Subcommand};
use jetstream_oxide::{
    DefaultJetstreamEndpoints, JetstreamCompression, JetstreamConfig, JetstreamConnector,
    events::{JetstreamEvent::Commit, commit::CommitEvent},
};
use serde::Deserialize;
use std::fs::File;
use std::io::Read;

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
    watch_did: String,
}

fn read_credentials() -> Result<Credentials, Box<dyn std::error::Error>> {
    let home_dir = std::env::var("HOME")?;
    let file_path = format!("{}/.pibot_login.json", home_dir);
    let mut file = File::open(file_path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;
    let credentials: Credentials = serde_json::from_str(&data)?;
    Ok(credentials)
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PiSearchResult {
    et: u64,
    r: Vec<ResultEntry>,
    status: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ResultEntry {
    k: String,
    st: u8,
    status: String,
    p: u64,
    db: String,
    da: String,
    c: u32,
}

async fn search_pi_str(number: &str) -> Result<PiSearchResult, Box<dyn std::error::Error>> {
    let url = format!("https://www.angio.net/newpi/piquery?q={}", number);
    let response = reqwest::get(&url).await?.text().await?;
    let result: PiSearchResult = serde_json::from_str(&response)?;
    Ok(result)
}

async fn search_pi(number: u32) -> Result<PiSearchResult, Box<dyn std::error::Error>> {
    let url = format!("https://www.angio.net/newpi/piquery?q={}", number);
    let response = reqwest::get(&url).await?.text().await?;
    let result: PiSearchResult = serde_json::from_str(&response)?;
    Ok(result)
}

fn generate_random_number() -> u32 {
    fastrand::u32(0..100_000_000)
}

fn get_today_date() -> String {
    chrono::Utc::now().format("%Y%m%d").to_string()
}

#[derive(clap::Parser)]
#[command(
    name = "pibot",
    version = "0.1.0",
    author = "David Andersen",
    about = "Posts Pi search results to Bsky"
)]
struct Cli {
    #[arg(long, help = "Be a BlueSky Bot. Commands: random, today, stream")]
    #[arg(
        long,
        short = 'n',
        help = "Dry run mode, just print the post content without actually posting"
    )]
    dry_run: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, PartialEq)]
enum Commands {
    Random,
    Today,
    Stream,
}

async fn post_to_bsky(
    agent: &BskyAgent,
    text: &str,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = RichText::new_with_detect_facets(text).await?;
    let record_data = RecordData {
        created_at: Datetime::now(),
        embed: None,
        entities: None,
        facets: rt.facets,
        labels: None,
        langs: None,
        reply: None,
        tags: Some(["#pi".to_string()].to_vec()),
        text: rt.text,
    };

    if dry_run {
        println!(
            "Dry run mode: The post content would be:\n{:?}",
            record_data
        );
    } else {
        agent.create_record(record_data).await?;
    }
    Ok(())
}

async fn streaming_mode(
    credentials: &Credentials,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent = BskyAgent::builder().build().await?;
    let _session = agent.login(&credentials.username, &credentials.password).await?;
    println!("Streaming mode!");
    let target_did = atrium_api::types::string::Did::new((&credentials.watch_did).into())?;
    let nsid = jetstream_oxide::exports::Nsid::new("app.bsky.feed.post".into()).unwrap();
    let config = JetstreamConfig {
        wanted_collections: vec![nsid],
        endpoint: DefaultJetstreamEndpoints::USEastTwo.into(),
        compression: JetstreamCompression::Zstd,
        ..Default::default()
    };

    let jetstream = JetstreamConnector::new(config).unwrap();
    let receiver = jetstream.connect().await?;

    while let Ok(event) = receiver.recv_async().await {
        if let Commit(CommitEvent::Create { info, commit, .. }) = event {
            let event_did = info.did.to_string();
            if let AppBskyFeedPost(record) = &commit.record {
                let matches = record.facets.as_ref().is_some_and(|facets| {
                    facets.iter().any(|facet| {
                        facet.data.features.iter().any(|feature| {
                            matches!(feature, Union::Refs(Mention(m)) if m.data.did == target_did)
                        })
                    })
                });
                if matches {
                    if !dry_run {
                        handle_message(&agent, &event_did, &commit).await;
                    } else {
                        println!("Dry run mode: Would handle message from {}", event_did);
                        println!("Message to post: {:#?}", record);
                    }
                }
            }
        }
    }
    println!("Exiting streaming mode!");
    Ok(())
}

pub fn extract_number(text: &str) -> Option<String> {
    let re = regex::Regex::new(r"@pisearch[^\d]*(\d[\d\-]*)").unwrap();
    re.captures(text)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .and_then(|s| {
            let number = s.replace("-", "");
            if number.is_empty() {
                None
            } else {
                Some(number)
            }
        })
}

pub async fn do_pisearch(text: &str) -> anyhow::Result<String> {
    let number = match extract_number(text) {
        Some(num) => num,
        None => {
            return Err(anyhow!("No number found"));
        }
    };
    println!("Searching for: {}", number);

    let search_result = match search_pi_str(&number).await {
        Ok(result) => result,
        Err(e) => {
            println!("Error searching: {e}");
            return Err(anyhow!("Error searching: {e}"));
        }
    };
    if search_result.status == "error" {
        return Err(anyhow!("Error searching pi"));
    }
    match search_result.r.first() {
        None =>
            Ok(format!("Sorry, I couldn't find {number} in the first 200m digits of Pi. It's me, not you; every number should be in Pi if I had more.")),
        Some(entry) => {
            if entry.status == "notfound" {
                Ok(format!("Sorry, I couldn't find {number} in the first 200m digits of Pi. It's me, not you; every number should be in Pi if I had more."))
            } else {
            Ok(format!(
        "I found {} at position {}. It appears {} times in the first 200 million digits of pi. Thanks for searching!\n\nFind all the #pi you can eat at https://angio.net/pi/",
        number,
        search_result.r.first().map_or(0, |entry| entry.p),
        search_result.r.len()
    ))
        }
    }
}
}

pub async fn handle_message(
    agent: &BskyAgent,
    did: &str,
    commit: &jetstream_oxide::events::commit::CommitData,
) {
    if let AppBskyFeedPost(record) = &commit.record {
        // Ugh - have to figure out if the post itself was a reply. If it was,
        // get the "root" field out of it and propagate it here.
        // Second ugh - we need to craft a URI for the post since it doesn't
        // seem to be included in the data.
        // at://<did>/app.bsky.feed.post/<rkey>
        let uri = format!(
            "at://{}/{}/{}",
            did,
            commit.info.collection.to_string(),
            commit.info.rkey
        );

        let reply_text = match do_pisearch(&record.text).await {
            Ok(text) => text,
            Err(e) => {
                println!("Error searching pi: {}", e);
                return;
            }
        };

        let root_data = match &record.reply {
            Some(data) => {
                strong_ref::MainData {
                cid: data.root.cid.clone(),
                uri: data.root.uri.clone(),
            }},
            _ => strong_ref::MainData {
                cid: commit.cid.clone(),
                uri: uri.clone(),
            },
        };

        let record_data = RecordData {
            created_at: Datetime::now(),
            embed: None,
            entities: None,
            facets: None,
            labels: None,
            langs: None,
            reply: Some(
                ReplyRefData {
                    root: root_data.into(),
                    parent: strong_ref::MainData {
                        cid: commit.cid.clone(),
                        uri,
                    }
                    .into(),
                }
                .into(),
            ),
            tags: Some(["#pi".to_string()].to_vec()),
            text: reply_text,
        };

        if let Err(e) = agent.create_record(record_data).await {
            println!("Error creating record: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let credentials = read_credentials()?;

    if cli.command == Commands::Stream {
        streaming_mode(&credentials, cli.dry_run).await?;
        return Ok(());
    }
    let number = match cli.command {
        Commands::Today => get_today_date().parse::<u32>()?,
        _ => generate_random_number(),
    };

    let search_result = search_pi(number).await?;

    let agent = BskyAgent::builder().build().await?;
    let _session = agent.login(&credentials.username, &credentials.password).await?;

    let post_content = match cli.command {
        Commands::Today => format!(
            "I found today in pi, {}, at position {}. It appears {} times in the first 200 million digits of pi.\n\nFind all the #pi you can eat at https://angio.net/pi/",
            number,
            search_result.r.first().map_or(0, |entry| entry.p),
            search_result.r.len()
        ),
        _ => format!(
            "The string {} was found at position {} in Pi. It appears {} times in the first 200 million digits of pi.\n\nFind all the #pi you can eat at https://angio.net/pi/",
            number,
            search_result.r.first().map_or(0, |entry| entry.p),
            search_result.r.len()
        ),
    };

    post_to_bsky(&agent, &post_content, cli.dry_run).await?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("@pisearch 12345"), Some("12345".to_string()));
        assert_eq!(extract_number("@pisearch 67890"), Some("67890".to_string()));
        assert_eq!(
            extract_number("@pisearch 123-456-7890"),
            Some("1234567890".to_string())
        );
        assert_eq!(extract_number("No number here"), None);
        assert_eq!(extract_number("@pisearch"), None);
        assert_eq!(extract_number("@pisearch -"), None);
        assert_eq!(extract_number("@pisearch 123-"), Some("123".to_string()));
        assert_eq!(
            extract_number("Hey @pisearch.bsky.social where is 2025-03-09 in Pi?"),
            Some("20250309".to_string())
        );
    }
}
