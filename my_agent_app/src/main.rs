use my_agent_sdk::Workspace;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("\x1b[1;36m=== 🛡️ RUST AGENT SDK SANDBOX SIMULATOR ===\x1b[0m\n");

    // Initialize Workspace at a local path "./safe_zone"
    let ws = Workspace::new("./safe_zone")?;
    println!("🚀 Workspace canonicalized at: \x1b[32m{:?}\x1b[0m", ws.root_dir());

    // Scenario 1: Legitimate Agent Operations
    println!("\n\x1b[1;33m--- [Scenario 1: Legitimate Agent Operations] ---\x1b[0m");
    
    // 1. Write file
    let test_file = "src/hello.rs";
    println!("✍️ Writing to '{}'...", test_file);
    ws.write_file(test_file, "fn main() {\n    println!(\"Hello World!\");\n}\n").await?;
    println!("✅ File written successfully!");

    // 2. Read file
    println!("📖 Reading '{}'...", test_file);
    let content = ws.read_file(test_file).await?;
    println!("📄 Content:\n\x1b[37m{}\x1b[0m", content);

    // 3. Edit file
    println!("✏️ Editing '{}' (replacing 'Hello World!' with 'Hello from Sandboxed Rust Agent!')...", test_file);
    ws.edit_file(test_file, "Hello World!", "Hello from Sandboxed Rust Agent!").await?;
    let updated_content = ws.read_file(test_file).await?;
    println!("📄 Updated Content:\n\x1b[37m{}\x1b[0m", updated_content);

    // 4. List files
    println!("🗂️ Listing files recursively under the root...");
    let files = ws.list_dir("").await?;
    for file in &files {
        println!("  - \x1b[34m{}\x1b[0m", file);
    }

    // Scenario 2: Traversal Hack Attempt
    println!("\n\x1b[1;33m--- [Scenario 2: Malicious Agent Traversal Hack] ---\x1b[0m");
    let bad_path = "../../../escape_test.txt";
    println!("🚨 Agent attempts to write to '{}'...", bad_path);
    match ws.write_file(bad_path, "Hacked content").await {
        Ok(_) => {
            println!("❌ \x1b[1;31mSECURITY FAILURE: Agent managed to escape sandbox!\x1b[0m");
        }
        Err(e) => {
            println!("🛡️ \x1b[1;32mSUCCESS: Escape attempt blocked!\x1b[0m Error details:");
            println!("   👉 \x1b[31m{}\x1b[0m", e);
        }
    }

    // Scenario 3: Absolute Path Hack Attempt
    println!("\n\x1b[1;33m--- [Scenario 3: Malicious Agent Absolute Path Hack] ---\x1b[0m");
    let abs_path = "/etc/passwd";
    println!("🚨 Agent attempts to read '{}'...", abs_path);
    match ws.read_file(abs_path).await {
        Ok(_) => {
            println!("❌ \x1b[1;31mSECURITY FAILURE: Agent read external system file!\x1b[0m");
        }
        Err(e) => {
            println!("🛡️ \x1b[1;32mSUCCESS: Read attempt blocked!\x1b[0m Error details:");
            println!("   👉 \x1b[31m{}\x1b[0m", e);
        }
    }

    println!("\n============================================");
    Ok(())
}
