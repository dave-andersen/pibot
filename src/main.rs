use atrium_api::app::bsky::feed::post::RecordData;
use atrium_api::types::string::Datetime;
use bsky_sdk::BskyAgent;
use bsky_sdk::rich_text::RichText;
use clap::{Parser, Subcommand};
use fastrand;
use reqwest;
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs::File;
use std::io::Read;
use tokio;

fn read_credentials() -> Result<(String, String), Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME")?;
    let file_path = format!("{}/.pibot_login.json", home_dir);
    let mut file = File::open(file_path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;
    let json: Value = serde_json::from_str(&data)?;
    let username = json["username"]
        .as_str()
        .ok_or("Username not found")?
        .to_string();
    let password = json["password"]
        .as_str()
        .ok_or("Password not found")?
        .to_string();
    Ok((username, password))
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
    #[arg(
        long,
        help = "Be a BlueSky Bot. Commands: random, today, stream"
    )]

    #[arg(
        long,
        short = 'n',
        help = "Dry run mode, just print the post content without actually posting"
    )]
    dry_run: bool,
    #[command(subcommand)]
    command: Commands
}

#[derive(Subcommand, PartialEq)]
enum Commands {
    Random,
    Today,
    Stream
}

async fn post_to_bsky(agent: &BskyAgent, text: &str, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
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
        println!("Dry run mode: The post content would be:\n{:?}", record_data);
    } else {
        agent.create_record(record_data).await?;
    }
    Ok(())
}


async fn streaming_mode(username: String, password: String, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let agent = BskyAgent::builder().build().await?;
    let _session = agent.login(&username, &password).await?;
    println!("Streaming mode!");
    println!("As you can tell, this doesn't exist yet.");
    println!("Exiting streaming mode!");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let (username, password) = read_credentials()?;

    if cli.command == Commands::Stream {
        streaming_mode(username, password, cli.dry_run).await?;
        return Ok(());
    }
    let number = match cli.command {
        Commands::Today => get_today_date().parse::<u32>()?,
        _ => generate_random_number(),
    };

    let search_result = search_pi(number).await?;

    let agent = BskyAgent::builder().build().await?;
    let _session = agent.login(&username, &password).await?;

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
