use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use eframe::egui;
use my_agent_sdk::{Workspace, ReadFileTool, WriteFileTool, EditFileTool, ListDirTool};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveTool {
    Read,
    Write,
    Edit,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Info,
    Success,
    Error,
}

#[derive(Clone)]
struct LogEntry {
    time: String,
    level: LogLevel,
    message: String,
    is_chat_response: bool,
}

impl LogEntry {
    fn new(level: LogLevel, message: String, is_chat_response: bool) -> Self {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        Self {
            time: now,
            level,
            message,
            is_chat_response,
        }
    }
    fn info(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Info, msg.into(), false)
    }
    fn success(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Success, msg.into(), false)
    }
    fn error(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Error, msg.into(), false)
    }
    fn chat_info(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Info, msg.into(), true)
    }
    fn chat_success(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Success, msg.into(), true)
    }
    fn chat_error(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Error, msg.into(), true)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SavedProject {
    name: String,
    folders: Vec<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SavedHistory {
    projects: Vec<SavedProject>,
    active_project_index: usize,
}

impl SavedHistory {
    fn load() -> Self {
        let path = Path::new("workspace_history.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(history) = serde_json::from_str::<SavedHistory>(&content) {
                    if !history.projects.is_empty() {
                        return history;
                    }
                }
            }
        }
        
        // Default history
        Self {
            projects: vec![SavedProject {
                name: "My Awesome Project".to_string(),
                folders: Vec::new(),
            }],
            active_project_index: 0,
        }
    }

    fn save(&self) {
        let path = Path::new("workspace_history.json");
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChatSender {
    User,
    System,
}

#[derive(Clone)]
struct ChatMessage {
    sender: ChatSender,
    time: String,
    text: String,
}

struct SandboxApp {
    history: SavedHistory,
    project_name: String,
    new_project_name_input: String,
    workspace: Workspace,
    selected_tool: ActiveTool,
    
    // Tab selector (0 = Manual, 1 = Chat CLI)
    active_tab: usize,
    
    // Manual tool inputs
    path_input: String,
    write_content: String,
    edit_find: String,
    edit_replace: String,
    list_path: String,
    
    // Chat inputs / history
    chat_input: String,
    chat_history: Vec<ChatMessage>,
    
    // Async communications
    logs: Vec<LogEntry>,
    tx: Sender<LogEntry>,
    rx: Receiver<LogEntry>,
}

impl SandboxApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let history = SavedHistory::load();
        let active_idx = history.active_project_index;
        let active_project = &history.projects[active_idx];
        let project_name = active_project.name.clone();
        let folders = active_project.folders.clone();
        
        let mut workspace = Workspace::new(&project_name);
        for folder in &folders {
            let _ = workspace.add_folder(folder);
        }
        
        let mut app = Self {
            history,
            project_name,
            new_project_name_input: "".to_string(),
            workspace,
            selected_tool: ActiveTool::Read,
            active_tab: 1, // Default to Chat CLI
            path_input: "test_file.txt".to_string(),
            write_content: "Hello from Sandboxed Workspace!".to_string(),
            edit_find: "Workspace".to_string(),
            edit_replace: "Rust Agent".to_string(),
            list_path: "".to_string(),
            chat_input: "".to_string(),
            chat_history: Vec::new(),
            logs: Vec::new(),
            tx,
            rx,
        };
        
        app.logs.push(LogEntry::info("Sandbox UI initialized successfully."));
        app.logs.push(LogEntry::info(format!("Loaded project '{}' with {} folders.", app.project_name, app.workspace.folders.len())));
        app
    }

    fn save_current_project_state(&mut self) {
        let idx = self.history.active_project_index;
        if idx < self.history.projects.len() {
            self.history.projects[idx].name = self.workspace.name.clone();
            self.history.projects[idx].folders = self.workspace.folders.clone();
            self.history.save();
        }
    }
}

impl eframe::App for SandboxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll for logs from background threads
        while let Ok(entry) = self.rx.try_recv() {
            if entry.is_chat_response {
                self.chat_history.push(ChatMessage {
                    sender: ChatSender::System,
                    time: entry.time.clone(),
                    text: entry.message.clone(),
                });
            }
            self.logs.push(entry);
        }

        // Apply a premium dark theme visual style
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(40, 120, 200);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 60, 70);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 34, 40);
        visuals.window_fill = egui::Color32::from_rgb(18, 20, 24);
        ui.ctx().set_visuals(visuals);

        // Sidebar Panel - Project Configuration
        egui::Panel::left("project_panel")
            .resizable(true)
            .default_size(320.0)
            .show_inside(ui, |ui| {
                ui.add_space(10.0);
                ui.heading("📂 Workspace Projects");
                ui.add_space(10.0);

                // Project List / Selector
                let mut active_idx = self.history.active_project_index;
                let mut project_changed = false;

                egui::ScrollArea::vertical().id_salt("projects_scroll").max_height(150.0).show(ui, |ui| {
                    for idx in 0..self.history.projects.len() {
                        ui.horizontal(|ui| {
                            let proj_name = &self.history.projects[idx].name;
                            let is_selected = idx == active_idx;
                            
                            if ui.selectable_label(is_selected, format!("📁 {}", proj_name)).clicked() {
                                active_idx = idx;
                                project_changed = true;
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.history.projects.len() > 1 {
                                    if ui.button("🗑️").on_hover_text("Delete Project").clicked() {
                                        self.history.projects.remove(idx);
                                        if active_idx >= self.history.projects.len() {
                                            active_idx = self.history.projects.len() - 1;
                                        }
                                        project_changed = true;
                                        self.history.save();
                                    }
                                }
                            });
                        });
                    }
                });

                if project_changed {
                    self.history.active_project_index = active_idx;
                    self.history.save();
                    
                    let selected_project = &self.history.projects[active_idx];
                    self.project_name = selected_project.name.clone();
                    self.workspace = Workspace::new(&selected_project.name);
                    for folder in &selected_project.folders {
                        let _ = self.workspace.add_folder(folder);
                    }
                    self.logs.push(LogEntry::info(format!("Switched to project: {}", self.project_name)));
                }

                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.new_project_name_input);
                    if ui.button("➕ New").clicked() {
                        let trimmed = self.new_project_name_input.trim().to_string();
                        if !trimmed.is_empty() {
                            let new_proj = SavedProject {
                                name: trimmed.clone(),
                                folders: Vec::new(),
                            };
                            self.history.projects.push(new_proj);
                            self.history.active_project_index = self.history.projects.len() - 1;
                            self.history.save();
                            
                            self.project_name = trimmed.clone();
                            self.workspace = Workspace::new(&trimmed);
                            self.new_project_name_input.clear();
                            self.logs.push(LogEntry::success(format!("Created new project: {}", trimmed)));
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    ui.label("Active Project Name:");
                    if ui.text_edit_singleline(&mut self.project_name).changed() {
                        self.workspace.name = self.project_name.clone();
                        self.save_current_project_state();
                    }
                });

                ui.add_space(10.0);
                ui.label("Registered Workspace Folders:");
                ui.add_space(5.0);

                let folders = self.workspace.folders.clone();
                egui::ScrollArea::vertical().id_salt("folders_scroll").max_height(180.0).show(ui, |ui| {
                    if folders.is_empty() {
                        ui.weak("No folders added yet. File tools will fail safety check.");
                    } else {
                        for folder in folders.iter() {
                            ui.horizontal(|ui| {
                                let path_str = folder.to_string_lossy();
                                let display_path = if path_str.len() > 30 {
                                    format!("...{}", &path_str[path_str.len() - 27..])
                                } else {
                                    path_str.to_string()
                                };
                                ui.label(egui::RichText::new(display_path).monospace()).on_hover_text(path_str.as_ref());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("❌").clicked() {
                                        self.workspace.remove_folder(folder);
                                        self.save_current_project_state();
                                        self.logs.push(LogEntry::info(format!("Removed folder: {:?}", folder)));
                                    }
                                });
                            });
                        }
                    }
                });

                ui.add_space(10.0);
                if ui.button("➕ Add Folder to Project").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        match self.workspace.add_folder(&path) {
                            Ok(_) => {
                                self.save_current_project_state();
                                self.logs.push(LogEntry::success(format!("Successfully added workspace folder: {:?}", path)));
                            }
                            Err(e) => {
                                self.logs.push(LogEntry::error(format!("Failed to add folder: {}", e)));
                            }
                        }
                    }
                }

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.weak("Sandbox Rules:");
                ui.weak("1. All operations are checked relative to the registered folders.");
                ui.weak("2. Absolute paths are allowed IF they resolve within a registered folder.");
                ui.weak("3. Traversal hacks (like ../../) will be canonicalized and blocked.");
            });

        // Central Panel - Tools Testing & Logs
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("🛡️ Sandboxed Agent CLI & Tools Suite");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.active_tab, 1, "💬 Interactive Chat CLI");
                    ui.selectable_value(&mut self.active_tab, 0, "🛠️ Manual Form Builder");
                });
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            if self.active_tab == 0 {
                // MANUAL FORM BUILDER TAB
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.selected_tool, ActiveTool::Read, "📖 Read File");
                    ui.selectable_value(&mut self.selected_tool, ActiveTool::Write, "✍️ Write File");
                    ui.selectable_value(&mut self.selected_tool, ActiveTool::Edit, "✏️ Edit File (Find & Replace)");
                    ui.selectable_value(&mut self.selected_tool, ActiveTool::List, "🗂️ List Files");
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                match self.selected_tool {
                    ActiveTool::Read => {
                        ui.label("Specify path to read (relative to workspace or absolute):");
                        ui.text_edit_singleline(&mut self.path_input);
                    }
                    ActiveTool::Write => {
                        ui.label("Specify path to write:");
                        ui.text_edit_singleline(&mut self.path_input);
                        ui.add_space(5.0);
                        ui.label("File Content:");
                        ui.text_edit_multiline(&mut self.write_content);
                    }
                    ActiveTool::Edit => {
                        ui.label("Specify path to edit:");
                        ui.text_edit_singleline(&mut self.path_input);
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label("Find Text:");
                                ui.text_edit_singleline(&mut self.edit_find);
                            });
                            ui.vertical(|ui| {
                                ui.label("Replace Text:");
                                ui.text_edit_singleline(&mut self.edit_replace);
                            });
                        });
                    }
                    ActiveTool::List => {
                        ui.label("Specify subdirectory to list (leave empty/'.' to list all folders recursively):");
                        ui.text_edit_singleline(&mut self.list_path);
                    }
                }

                ui.add_space(15.0);

                let button_text = match self.selected_tool {
                    ActiveTool::Read => "Run ReadFileTool",
                    ActiveTool::Write => "Run WriteFileTool",
                    ActiveTool::Edit => "Run EditFileTool",
                    ActiveTool::List => "Run ListDirTool",
                };

                let tx = self.tx.clone();
                let ws = self.workspace.clone();
                
                if ui.button(egui::RichText::new(button_text).heading()).clicked() {
                    let tool = self.selected_tool;
                    let path_in = PathBuf::from(self.path_input.trim());
                    let content_in = self.write_content.clone();
                    let find_in = self.edit_find.clone();
                    let replace_in = self.edit_replace.clone();
                    let list_path_in = PathBuf::from(self.list_path.trim());

                    tokio::spawn(async move {
                        match tool {
                            ActiveTool::Read => {
                                let tool = ReadFileTool::new();
                                tx.send(LogEntry::info(format!("Executing ReadFileTool for path: {:?}", path_in))).unwrap();
                                match tool.run(&ws, &path_in).await {
                                    Ok(content) => {
                                        tx.send(LogEntry::success(format!("Read File Success!\n--- CONTENT ---\n{}\n---------------", content))).unwrap();
                                    }
                                    Err(e) => {
                                        tx.send(LogEntry::error(format!("Security / Read Error: {}", e))).unwrap();
                                    }
                                }
                            }
                            ActiveTool::Write => {
                                let tool = WriteFileTool::new();
                                tx.send(LogEntry::info(format!("Executing WriteFileTool for path: {:?}", path_in))).unwrap();
                                match tool.run(&ws, &path_in, &content_in).await {
                                    Ok(_) => {
                                        tx.send(LogEntry::success("Write File Success!")).unwrap();
                                    }
                                    Err(e) => {
                                        tx.send(LogEntry::error(format!("Security / Write Error: {}", e))).unwrap();
                                    }
                                }
                            }
                            ActiveTool::Edit => {
                                let tool = EditFileTool::new();
                                tx.send(LogEntry::info(format!("Executing EditFileTool (replacing '{}' with '{}') for path: {:?}", find_in, replace_in, path_in))).unwrap();
                                match tool.run(&ws, &path_in, &find_in, &replace_in).await {
                                    Ok(_) => {
                                        tx.send(LogEntry::success("Edit File Success!")).unwrap();
                                    }
                                    Err(e) => {
                                        tx.send(LogEntry::error(format!("Security / Edit Error: {}", e))).unwrap();
                                    }
                                }
                            }
                            ActiveTool::List => {
                                let tool = ListDirTool::new();
                                tx.send(LogEntry::info(format!("Executing ListDirTool for path: {:?}", list_path_in))).unwrap();
                                match tool.run(&ws, &list_path_in).await {
                                    Ok(files) => {
                                        let mut out = format!("List Workspace Success ({} entries found):\n", files.len());
                                        for f in files {
                                            out.push_str(&format!("  - {}\n", f.to_string_lossy()));
                                        }
                                        tx.send(LogEntry::success(out)).unwrap();
                                    }
                                    Err(e) => {
                                        tx.send(LogEntry::error(format!("Security / List Error: {}", e))).unwrap();
                                    }
                                }
                            }
                        }
                    });
                }
            } else {
                // INTERACTIVE CHAT CLI TAB
                let chat_scroll_height = ui.available_height() - 120.0;
                
                egui::ScrollArea::vertical()
                    .id_salt("chat_scroll")
                    .max_height(chat_scroll_height)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.chat_history.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.weak("Interactive Chat CLI is ready.");
                                ui.weak("Type /help to see available commands.");
                                ui.weak("Example: `/readfile a.txt @x` to read file specifically from folder tagged @x.");
                            });
                        } else {
                            for msg in &self.chat_history {
                                match msg.sender {
                                    ChatSender::User => {
                                        ui.horizontal(|ui| {
                                            ui.weak(format!("[{}] User:", msg.time));
                                            ui.label(egui::RichText::new(&msg.text).strong().color(egui::Color32::from_rgb(100, 200, 255)));
                                        });
                                    }
                                    ChatSender::System => {
                                        ui.horizontal(|ui| {
                                            ui.weak(format!("[{}] System:", msg.time));
                                            ui.label(egui::RichText::new(&msg.text).monospace());
                                        });
                                    }
                                }
                                ui.add_space(5.0);
                            }
                        }
                    });

                ui.separator();

                // Suggestion helper buttons for tags
                let folders_list = self.workspace.folders.clone();
                if !folders_list.is_empty() {
                    ui.horizontal(|ui| {
                        ui.weak("Tag suggestions (click to insert):");
                        for folder in &folders_list {
                            if let Some(name) = folder.file_name() {
                                let tag_name = name.to_string_lossy();
                                let tag_btn_text = format!("@{}", tag_name);
                                if ui.button(&tag_btn_text).clicked() {
                                    self.chat_input.push_str(&format!(" @{}", tag_name));
                                }
                            }
                        }
                    });
                }

                // Chat Input bar
                ui.horizontal(|ui| {
                    let text_edit = ui.text_edit_singleline(&mut self.chat_input);
                    let enter_pressed = text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    
                    if (ui.button("Send").clicked() || enter_pressed) && !self.chat_input.trim().is_empty() {
                        let input = self.chat_input.trim().to_string();
                        self.chat_input.clear();

                        self.chat_history.push(ChatMessage {
                            sender: ChatSender::User,
                            time: chrono::Local::now().format("%H:%M:%S").to_string(),
                            text: input.clone(),
                        });

                        let tx = self.tx.clone();
                        let ws = self.workspace.clone();

                        tokio::spawn(async move {
                            if input.starts_with("/readfile") {
                                let rest = input.trim_start_matches("/readfile").trim();
                                if rest.is_empty() {
                                    tx.send(LogEntry::chat_error("Usage: /readfile <filename> [@folder]")).unwrap();
                                    return;
                                }
                                
                                // Parse tag if present
                                let (filename, tag) = if let Some(pos) = rest.find('@') {
                                    (rest[..pos].trim(), Some(rest[pos+1..].trim()))
                                } else {
                                    (rest, None)
                                };
                                
                                if let Some(folder_tag) = tag {
                                    let mut matched_folder = None;
                                    for folder in &ws.folders {
                                        if let Some(name) = folder.file_name() {
                                            if name.to_string_lossy() == folder_tag {
                                                matched_folder = Some(folder.clone());
                                                break;
                                            }
                                        }
                                    }
                                    
                                    match matched_folder {
                                        Some(folder_path) => {
                                            let tool = ReadFileTool::new();
                                            let full_path = folder_path.join(filename);
                                            tx.send(LogEntry::chat_info(format!("Reading {:?} from @{}", filename, folder_tag))).unwrap();
                                            match tool.run(&ws, &full_path).await {
                                                Ok(content) => {
                                                    tx.send(LogEntry::chat_success(format!("--- CONTENT OF @{}/{} ---\n{}\n------------------", folder_tag, filename, content))).unwrap();
                                                }
                                                Err(e) => {
                                                    tx.send(LogEntry::chat_error(format!("Failed to read file: {}", e))).unwrap();
                                                }
                                            }
                                        }
                                        None => {
                                            tx.send(LogEntry::chat_error(format!("Folder tag '@{}' not found in workspace. Register folder first.", folder_tag))).unwrap();
                                        }
                                    }
                                } else {
                                    let mut matching_folders = Vec::new();
                                    for folder in &ws.folders {
                                        let full_path = folder.join(filename);
                                        if full_path.exists() && full_path.is_file() {
                                            if let Some(name) = folder.file_name() {
                                                matching_folders.push((name.to_string_lossy().into_owned(), folder.clone()));
                                            }
                                        }
                                    }
                                    
                                    if matching_folders.is_empty() {
                                        if ws.folders.is_empty() {
                                            tx.send(LogEntry::chat_error("Workspace has no folders configured.")).unwrap();
                                        } else {
                                            let tool = ReadFileTool::new();
                                            let full_path = PathBuf::from(filename);
                                            match tool.run(&ws, &full_path).await {
                                                Ok(content) => {
                                                    tx.send(LogEntry::chat_success(format!("--- CONTENT OF {} ---\n{}\n------------------", filename, content))).unwrap();
                                                }
                                                Err(e) => {
                                                    tx.send(LogEntry::chat_error(format!("File not found in any folders: {}", e))).unwrap();
                                                }
                                            }
                                        }
                                    } else if matching_folders.len() == 1 {
                                        let (folder_tag, folder_path) = &matching_folders[0];
                                        let tool = ReadFileTool::new();
                                        let full_path = folder_path.join(filename);
                                        match tool.run(&ws, &full_path).await {
                                            Ok(content) => {
                                                tx.send(LogEntry::chat_success(format!("--- CONTENT OF @{}/{} ---\n{}\n------------------", folder_tag, filename, content))).unwrap();
                                            }
                                            Err(e) => {
                                                tx.send(LogEntry::chat_error(format!("Failed to read file: {}", e))).unwrap();
                                            }
                                        }
                                    } else {
                                        let mut msg = format!("Ambiguity detected! The file '{}' exists in multiple folders:\n", filename);
                                        for (tag, _) in &matching_folders {
                                            msg.push_str(&format!("  - @{}\n", tag));
                                        }
                                        msg.push_str(&format!("\nPlease specify which one you want to read, e.g.:\n  `/readfile {} @{}`", filename, matching_folders[0].0));
                                        tx.send(LogEntry::chat_error(msg)).unwrap();
                                    }
                                }
                            } else if input.starts_with("/list") {
                                let rest = input.trim_start_matches("/list").trim();
                                let tag = if rest.starts_with('@') {
                                    Some(rest.trim_start_matches('@').trim())
                                } else {
                                    None
                                };

                                let tool = ListDirTool::new();
                                if let Some(folder_tag) = tag {
                                    let mut matched_folder = None;
                                    for folder in &ws.folders {
                                        if let Some(name) = folder.file_name() {
                                            if name.to_string_lossy() == folder_tag {
                                                matched_folder = Some(folder.clone());
                                                break;
                                            }
                                        }
                                    }

                                    match matched_folder {
                                        Some(folder_path) => {
                                            tx.send(LogEntry::chat_info(format!("Listing files in @{}", folder_tag))).unwrap();
                                            match tool.run(&ws, &folder_path).await {
                                                Ok(files) => {
                                                    let mut out = format!("Files in @{} ({} found):\n", folder_tag, files.len());
                                                    for f in files {
                                                        if let Ok(rel) = f.strip_prefix(&folder_path) {
                                                            out.push_str(&format!("  - {}\n", rel.to_string_lossy()));
                                                        } else {
                                                            out.push_str(&format!("  - {}\n", f.to_string_lossy()));
                                                        }
                                                    }
                                                    tx.send(LogEntry::chat_success(out)).unwrap();
                                                }
                                                Err(e) => {
                                                    tx.send(LogEntry::chat_error(format!("List error: {}", e))).unwrap();
                                                }
                                            }
                                        }
                                        None => {
                                            tx.send(LogEntry::chat_error(format!("Folder tag '@{}' not found.", folder_tag))).unwrap();
                                        }
                                    }
                                } else {
                                    tx.send(LogEntry::chat_info("Listing files across all folders...")).unwrap();
                                    match tool.run(&ws, Path::new("")).await {
                                        Ok(files) => {
                                            let mut out = format!("All Workspace Files ({} found):\n", files.len());
                                            for f in files {
                                                let mut display_name = f.to_string_lossy().into_owned();
                                                for folder in &ws.folders {
                                                    if let Ok(rel) = f.strip_prefix(folder) {
                                                        if let Some(name) = folder.file_name() {
                                                            display_name = format!("@{}/{}", name.to_string_lossy(), rel.to_string_lossy());
                                                        }
                                                        break;
                                                    }
                                                }
                                                out.push_str(&format!("  - {}\n", display_name));
                                            }
                                            tx.send(LogEntry::chat_success(out)).unwrap();
                                        }
                                        Err(e) => {
                                            tx.send(LogEntry::chat_error(format!("List error: {}", e))).unwrap();
                                        }
                                    }
                                }
                            } else if input.starts_with("/help") {
                                let mut help_msg = "Available CLI Commands:\n".to_string();
                                help_msg.push_str("  - `/readfile <file_name>` : Reads file. Warns if it is ambiguous/exists in multiple folders.\n");
                                help_msg.push_str("  - `/readfile <file_name> @<folder>` : Reads file specifically from the tagged folder.\n");
                                help_msg.push_str("  - `/list` : Lists all files across all registered folders.\n");
                                help_msg.push_str("  - `/list @<folder>` : Lists all files in a specific folder.\n");
                                help_msg.push_str("  - `/help` : Displays this helper menu.");
                                tx.send(LogEntry::chat_info(help_msg)).unwrap();
                            } else {
                                tx.send(LogEntry::chat_info(format!("Unknown command. Type `/help` for list of commands. Entered: '{}'", input))).unwrap();
                            }
                        });
                    }
                });
            }

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(5.0);

            // Developer Log Console
            ui.horizontal(|ui| {
                ui.heading("💻 Global Developer Console");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear Console").clicked() {
                        self.logs.clear();
                    }
                });
            });

            ui.add_space(5.0);

            egui::ScrollArea::vertical()
                .id_salt("console_scroll")
                .max_height(140.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.logs.is_empty() {
                        ui.weak("Console is empty.");
                    } else {
                        for log in &self.logs {
                            let color = match log.level {
                                LogLevel::Info => egui::Color32::from_rgb(180, 180, 180),
                                LogLevel::Success => egui::Color32::from_rgb(46, 204, 113),
                                LogLevel::Error => egui::Color32::from_rgb(231, 76, 60),
                            };
                            let prefix = match log.level {
                                LogLevel::Info => "[INFO]",
                                LogLevel::Success => "[SUCCESS]",
                                LogLevel::Error => "[ERROR]",
                            };
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("[{}]", log.time)).weak());
                                ui.label(egui::RichText::new(prefix).color(color).strong());
                                ui.label(egui::RichText::new(&log.message).color(color).monospace());
                            });
                        }
                    }
                });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    let _guard = rt.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("🛡️ Rust Agent Sandbox GUI")
            .with_inner_size([1100.0, 750.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Rust Agent Sandbox GUI",
        options,
        Box::new(|cc| {
            Ok(Box::new(SandboxApp::new(cc)))
        }),
    )
}
