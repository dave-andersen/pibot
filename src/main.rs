use bsky_sdk::BskyAgent;
use bsky_sdk::rich_text::RichText;
use std::fs::File;
use std::io::Read;
use serde_json::Value;
use fastrand;
use std::env;
use tokio;
use serde::Deserialize;
use atrium_api::types::string::Datetime;
use atrium_api::app::bsky::feed::post::RecordData;

fn read_credentials() -> Result<(String, String), Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME")?;
    let file_path = format!("{}/.pibot_login.json", home_dir);
    let mut file = File::open(file_path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;
    let json: Value = serde_json::from_str(&data)?;
    let username = json["username"].as_str().ok_or("Username not found")?.to_string();
    let password = json["password"].as_str().ok_or("Password not found")?.to_string();
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (username, password) = read_credentials()?;

    let random_number = generate_random_number();
    let search_result = search_pi(random_number).await?;

    let agent = BskyAgent::builder().build().await?;
    let _session = agent.login(&username, &password).await?;

    if let Some(first_entry) = search_result.r.first() {
        let post_content = format!(
            "The string {} was found at position {} in Pi. It appears {} times in the first 200 million digits of pi.\n\nFind all the pi you can eat at https://angio.net/pi/",
            random_number, first_entry.p, search_result.r.len()
        );

        let rt = RichText::new_with_detect_facets(&post_content).await?;
        
        agent
            .create_record(RecordData {
                created_at: Datetime::now(),
                embed: None,
                entities: None,
                facets: rt.facets,
                labels: None,
                langs: None,
                reply: None,
                tags: None,
                text: rt.text,
            })
            .await?;
    }
    

    Ok(())
}
