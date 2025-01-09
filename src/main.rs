#![windows_subsystem = "windows"]

use eframe::egui::{self, Color32, Key, TextEdit, Vec2, ComboBox};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::fs;
use std::fs::File; // Para manipulação de arquivos
use serde_json; // Para serialização para JSON
use std::io::{self, BufRead};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Channel {
    name: String,
    status: bool,
    open_in_browser: bool,
    opened_in_browser: bool, // Flag para verificar se já foi aberto no navegador
}

#[derive(Default, Serialize, Deserialize)]
struct AppState {
    channels: Vec<Channel>, // Usando Vec para manter a ordem de inserção
}

struct TwitchMonitorApp {
    state: Arc<Mutex<AppState>>,
    new_channel: String,
    client: Client,
    headers: HeaderMap,
    file_path: String, // Caminho do arquivo JSON
}

impl TwitchMonitorApp {
    fn new(client_id: &str, access_token: &str, file_path: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("Client-Id", HeaderValue::from_str(client_id).unwrap());
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap());

        let state = Arc::new(Mutex::new(AppState::default()));

        // Carregar o estado dos canais a partir do arquivo JSON, se ele existir
        if let Ok(json_data) = fs::read_to_string(file_path.as_str())
        {
            match serde_json::from_str::<Vec<Channel>>(&json_data) {
                Ok(loaded_state) => {
                    let mut state_lock = state.lock().unwrap();
                    state_lock.channels = loaded_state;
                }
                Err(e) => eprintln!("Erro ao carregar canais do arquivo JSON: {}", e),
            }
        }

        Self {
            state,
            new_channel: String::new(),
            client: Client::new(),
            headers,
            file_path,
        }
    }

    fn add_channel(&mut self) {
        if !self.new_channel.trim().is_empty() {
            let mut state = self.state.lock().unwrap();
            state.channels.push(Channel {
                name: self.new_channel.trim().to_string(),
                status: false, // Inicializando como offline
                open_in_browser: false, // Inicializando como false
                opened_in_browser: false, // Inicializando como false
            });
            self.new_channel.clear();
            Self::save_channels_to_file(&state.channels, &self.file_path);
        }
    }

    fn check_channels(state: Arc<Mutex<AppState>>, client: Client, headers: HeaderMap, file_path: String) {
        thread::spawn(move || loop {
            let channels: Vec<String> = {
                let state = state.lock().unwrap();
                state.channels.iter().map(|c| c.name.clone()).collect()
            };

            if !channels.is_empty() {
                let url = format!("https://api.twitch.tv/helix/streams?user_login={}", channels.join("&user_login="));
                if let Ok(response) = client
                    .get(&url)
                    .headers(headers.clone()) // Usar os headers já configurados
                    .send() {
                    if let Ok(data) = response.json::<TwitchStreamsResponse>() {
                        let mut state = state.lock().unwrap();
                        for channel in state.channels.iter_mut() {
                            let was_online = channel.status;
                            channel.status = data.data.iter().any(|stream| stream.user_login == channel.name);
                        
                            // Verificar se o status mudou de offline para online
                            if channel.status && !was_online && channel.open_in_browser && !channel.opened_in_browser {
                                let _ = open::that(format!("https://www.twitch.tv/{}", channel.name));
                                channel.opened_in_browser = true; // Marcar como aberto
                            }
                        
                            // Se o canal estiver offline, resetar a flag
                            if !channel.status {
                                channel.opened_in_browser = false;
                            }
                        }

                        // Salvar estado dos canais no arquivo JSON
                        Self::save_channels_to_file(&state.channels, &file_path);
                    }
                }
            }

            thread::sleep(Duration::from_secs(30));
        });
    }

    fn remove_channel(&mut self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.channels.retain(|channel| channel.name != name);
        Self::save_channels_to_file(&state.channels, &self.file_path);
    }

    // Função para salvar os canais no arquivo JSON
    fn save_channels_to_file(channels: &Vec<Channel>, file_path: &str) {
        // Convertendo o Vec para JSON e salvando no arquivo
        let json_data = serde_json::to_string(channels).unwrap();
        fs::write(file_path, json_data).expect("Unable to write file");
    }
}

impl eframe::App for TwitchMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Twitch Channel Monitor");

            // Campo de entrada para adicionar novos canais
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.new_channel).hint_text("Adicionar canal..."));
                if ui.button("Adicionar").clicked() || ctx.input(|i| i.key_pressed(Key::Enter)) {
                    self.add_channel();
                }
            });

            ui.separator();

            // Vetor para armazenar os canais a serem removidos
            let mut channels_to_remove = Vec::new();

            {
                let mut state = self.state.lock().unwrap();
                for channel in &mut state.channels {
                    ui.horizontal(|ui| {
                        let status_color = if channel.status {
                            Color32::GREEN
                        } else {
                            Color32::RED
                        };
                        ui.colored_label(status_color, if channel.status { "⬤" } else { "⬤" });

                        if ui.add(egui::Label::new(&channel.name).sense(egui::Sense::click())).clicked() {
                            let _ = open::that(format!("https://www.twitch.tv/{}", channel.name));
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Label::new("❌").sense(egui::Sense::click())).clicked() {
                                channels_to_remove.push(channel.name.clone());
                            }
                
                            ui.horizontal(|ui| {
                                // Criando uma ComboBox para selecionar "Sim" ou "Não"
                                ComboBox::from_id_source(channel.name.clone()) // Usa o nome do canal como id único
                                    .selected_text(if channel.open_in_browser { "Abrir no navegador" } else { "Não abrir no navegador" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut channel.open_in_browser, true, "Abrir no navegador");
                                        ui.selectable_value(&mut channel.open_in_browser, false, "Não abrir no navegador");
                                    });
                            });
                        });
                    });
                }
            }

            // Remover os canais fora do fechamento
            for channel_name in channels_to_remove {
                self.remove_channel(&channel_name);
            }
        });

        ctx.request_repaint_after(Duration::from_millis(16)); // Atualização suave
    }
}

fn read_credentials() -> io::Result<(String, String)> {
    // Caminho relativo à pasta release
    let relative_path: PathBuf = ["..", "..", "env", "auth.txt"].iter().collect();

    // Resolve o caminho absoluto
    let file_path = fs::canonicalize(&relative_path).expect("Erro ao resolver o caminho");

    let file = File::open(file_path)?;
    let reader = io::BufReader::new(file);

    let mut client_id = String::new();
    let mut oauth_token = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed_line = line.trim();

        if trimmed_line.starts_with("client_id =") {
            client_id = trimmed_line.split('=').nth(1).unwrap().trim().to_string();
        } else if trimmed_line.starts_with("oauth_token =") {
            oauth_token = trimmed_line.split('=').nth(1).unwrap().trim().to_string();
        }
    }

    Ok((client_id, oauth_token))
}

fn main() -> Result<(), eframe::Error> {
    let (client_id, oauth_token) = read_credentials()
        .expect("Erro ao ler o arquivo de credenciais");


    let file_path = "channels.json".to_string(); // Caminho do arquivo JSON

    let options = eframe::NativeOptions {
        initial_window_size: Some(Vec2::new(400.0, 600.0)),
        ..Default::default()
    };
    let app = TwitchMonitorApp::new(&client_id, &oauth_token, file_path.clone());
    TwitchMonitorApp::check_channels(app.state.clone(), app.client.clone(), app.headers.clone(), file_path);
    eframe::run_native("Twitch Monitor", options, Box::new(|_cc| Box::new(app)))
}

#[derive(Deserialize, Debug)]
struct TwitchStream {
    user_login: String,
}

#[derive(Deserialize, Debug)]
struct TwitchStreamsResponse {
    data: Vec<TwitchStream>,
}
