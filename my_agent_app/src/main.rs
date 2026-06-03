use std::path::PathBuf;
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
}

impl LogEntry {
    fn new(level: LogLevel, message: String) -> Self {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        Self {
            time: now,
            level,
            message,
        }
    }
    fn info(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Info, msg.into())
    }
    fn success(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Success, msg.into())
    }
    fn error(msg: impl Into<String>) -> Self {
        Self::new(LogLevel::Error, msg.into())
    }
}

struct SandboxApp {
    project_name: String,
    workspace: Workspace,
    selected_tool: ActiveTool,
    
    // Tool inputs
    path_input: String,
    write_content: String,
    edit_find: String,
    edit_replace: String,
    list_path: String,
    
    // Async communications
    logs: Vec<LogEntry>,
    tx: Sender<LogEntry>,
    rx: Receiver<LogEntry>,
}

impl SandboxApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let default_project = "My Awesome Project";
        let workspace = Workspace::new(default_project);
        
        let mut app = Self {
            project_name: default_project.to_string(),
            workspace,
            selected_tool: ActiveTool::Read,
            path_input: "test_file.txt".to_string(),
            write_content: "Hello from Sandboxed Workspace!".to_string(),
            edit_find: "Workspace".to_string(),
            edit_replace: "Rust Agent".to_string(),
            list_path: "".to_string(),
            logs: Vec::new(),
            tx,
            rx,
        };
        
        app.logs.push(LogEntry::info("Sandbox UI initialized successfully."));
        app.logs.push(LogEntry::info("Add some folders to get started!"));
        app
    }
}

impl eframe::App for SandboxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll for logs from background threads
        while let Ok(entry) = self.rx.try_recv() {
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
                ui.heading("📂 Project / Workspace");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Project Name:");
                    if ui.text_edit_singleline(&mut self.project_name).changed() {
                        self.workspace.name = self.project_name.clone();
                    }
                });

                ui.separator();
                ui.add_space(5.0);
                
                ui.label("Registered Workspace Folders:");
                ui.add_space(5.0);

                let folders = self.workspace.folders.clone();
                egui::ScrollArea::vertical().id_salt("folders_scroll").max_height(250.0).show(ui, |ui| {
                    if folders.is_empty() {
                        ui.weak("No folders added yet. File tools will fail safety check.");
                    } else {
                        for folder in folders.iter() {
                            ui.horizontal(|ui| {
                                let path_str = folder.to_string_lossy();
                                // Trim path to display nicely
                                let display_path = if path_str.len() > 30 {
                                    format!("...{}", &path_str[path_str.len() - 27..])
                                } else {
                                    path_str.to_string()
                                };
                                ui.label(egui::RichText::new(display_path).monospace());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("❌").clicked() {
                                        self.workspace.remove_folder(folder);
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
            ui.heading("🛡️ Sandboxed Agent Tools Suite");
            ui.add_space(15.0);

            // Tool Selector (Tabs)
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tool, ActiveTool::Read, "📖 Read File");
                ui.selectable_value(&mut self.selected_tool, ActiveTool::Write, "✍️ Write File");
                ui.selectable_value(&mut self.selected_tool, ActiveTool::Edit, "✏️ Edit File (Find & Replace)");
                ui.selectable_value(&mut self.selected_tool, ActiveTool::List, "🗂️ List Files");
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Dynamic Inputs depending on Tool selected
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

            // Execute Button
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

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Log Console
            ui.horizontal(|ui| {
                ui.heading("💻 Execution Console Output");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear Logs").clicked() {
                        self.logs.clear();
                    }
                });
            });

            ui.add_space(5.0);

            // Scrollable Console area
            egui::ScrollArea::vertical()
                .id_salt("console_scroll")
                .max_height(280.0)
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
                                LogLevel::Error => "[BLOCKED/ERROR]",
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("🛡️ Rust Agent Sandbox GUI")
            .with_inner_size([1000.0, 700.0]),
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
