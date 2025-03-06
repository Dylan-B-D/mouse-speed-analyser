#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui::Vec2b;
use egui_plot::{Line, Plot, PlotPoints};
use multiinput::{DeviceType, RawEvent, RawInputManager};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// State for app
#[derive(Default)]
struct MouseState {
    events_count: usize,                                // Number of events in the current interval
    events_per_second: usize,                           // Number of events per second
    delta: (i32, i32),                                  // Mouse delta
    running: bool,                                      // Flag to control the thread
    max_speed: f64,                                     // Maximum speed
    dpi: f64,                                           // DPI
    last_event_time: Option<Instant>,                   // Time of the last event
    speed_history: VecDeque<(f64, f64)>,                // History of speed over time
    polling_history: VecDeque<(f64, f64)>,              // History of polling rate over time
    start_time: Option<Instant>,                        // Start time
    event_history: VecDeque<(f64, (i32, i32))>,         // History of mouse events over time
    last_polling_update: Option<Instant>,               // Time of the last polling update
}

// App struct for UI input fields and state
struct MouseApp {
    state: Arc<Mutex<MouseState>>,                      // Shared state
    dpi_string: String,                                 // DPI input string
    window_duration_string: String,                     // Window duration input string
}

impl MouseApp {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(MouseState {
            events_count: 0,
            events_per_second: 0,
            delta: (0, 0),
            running: true,
            max_speed: 0.0,
            dpi: 1600.0,
            last_event_time: None,
            speed_history: VecDeque::with_capacity(1000),
            polling_history: VecDeque::with_capacity(1000),
            start_time: Some(Instant::now()),
            event_history: VecDeque::with_capacity(1000),
            last_polling_update: Some(Instant::now()),
        }));

        let dpi_string = "1600.0".to_string();                        // Default DPI
        let window_duration_string = "5.0".to_string();               // Default window duration for averaging speed
        let state_clone = state.clone();              // Clone for the polling thread
        let polling_interval = Duration::from_millis(15);           // Interval for polling rate updates

        // Mouse event handling thread
        thread::spawn(move || {
            let mut manager = RawInputManager::new().unwrap();            // Raw input manager
            manager.register_devices(DeviceType::Mice);                                    // Register mice

            loop {
                {
                    // Check if the application is still running - if not, exit the thread
                    if !state_clone.lock().unwrap().running {
                        break;
                    }
                }

                // Poll for mouse events
                if let Some(event) = manager.get_event() {
                    if let RawEvent::MouseMoveEvent(_, x, y) = event {
                        let now = Instant::now();
                        let mut state = state_clone.lock().unwrap();
                        let elapsed_time = now.duration_since(state.start_time.unwrap()).as_secs_f64();

                        // Calculate speed
                        state.last_event_time = Some(now);
                        state.events_count += 1;
                        state.delta = (x, y);
                        state.event_history.push_back((elapsed_time, (x, y)));
                    }
                } else {
                    // If no events, sleep for a short time to avoid busy waiting
                    thread::sleep(Duration::from_micros(100));
                }
            }
        });

        // Thread for continuous polling rate updates
        let state_clone = state.clone();
        thread::spawn(move || {
            loop {
                {
                    let mut state = state_clone.lock().unwrap();
                    if !state.running {
                        break;
                    }
                    
                    let now = Instant::now();
                    let elapsed_time = now.duration_since(state.start_time.unwrap()).as_secs_f64();
                    
                    // Update polling rate every polling_interval
                    if let Some(last_update) = state.last_polling_update {
                        if last_update.elapsed() >= polling_interval {
                            state.events_per_second = (state.events_count as f64 
                                * (1.0 / polling_interval.as_secs_f64())) as usize;
                            let events = state.events_per_second as f64;
                            state.polling_history.push_back((elapsed_time, events));
                            
                            if state.polling_history.len() > 1000 {
                                state.polling_history.pop_front();
                            }
                            
                            state.events_count = 0;
                            state.last_polling_update = Some(now);
                        }
                    }
                }
                thread::sleep(polling_interval);
            }
        });

        Self { 
            state, 
            dpi_string,
            window_duration_string,
        }
    }
}

impl eframe::App for MouseApp {
    // Update function for UI
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let current_time = now.duration_since(state.start_time.unwrap()).as_secs_f64();

        // Prune histories based on graph_duration
        while let Some(&(t, _)) = state.polling_history.front() {
            if current_time - t > 5.0 {
                state.polling_history.pop_front();
            } else {
                break;
            }
        }

        // Prune speed history based on graph_duration
        while let Some(&(t, _)) = state.speed_history.front() {
            if current_time - t > 5.0 {
                state.speed_history.pop_front();
            } else {
                break;
            }
        }

        // Parse window_duration from input string (convert from ms to seconds)
        let window_duration = self.window_duration_string.parse::<f64>().unwrap_or(5.0) / f64::max(1000.0, 0.0001); // Minimum 0.1ms
        
        // Use the adjustable window_duration to prune event history and compute speed
        while let Some(&(t, _)) = state.event_history.front() {
            if current_time - t > window_duration {
                state.event_history.pop_front();
            } else {
                break;
            }
        }

        // Calculate speed based on the current window_duration
        let mut total_delta_counts = 0.0;
        for &(_, (dx, dy)) in state.event_history.iter() {
            let delta = ((dx * dx + dy * dy) as f64).sqrt();                // Calculate the distance moved
            total_delta_counts += delta;                                         // Accumulate counts
        }

        let meters_per_count = 0.0254 / state.dpi;                          // 1 inch = 0.0254 meters
        let total_distance = total_delta_counts * meters_per_count;         // Convert counts to meters
        // Calculate speed
        let speed = if !state.event_history.is_empty() {
            total_distance / window_duration
        } else {
            0.0
        };

        // Update max speed
        if speed > state.max_speed {
            state.max_speed = speed;
        }

        // Update speed history
        state.speed_history.push_back((current_time, speed));
        if state.speed_history.len() > 1000 {
            state.speed_history.pop_front();
        }

        // Update UI
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Mouse Speed Analyser");

            ui.horizontal(|ui| {
                ui.label("DPI:");
                let dpi_response = ui.add(
                    egui::TextEdit::singleline(&mut self.dpi_string)
                        .desired_width(60.0)
                        .hint_text("Enter DPI"),
                );
                if dpi_response.changed() {
                    if let Ok(new_dpi) = self.dpi_string.parse::<f64>() {
                        if new_dpi > 0.0 {
                            state.dpi = new_dpi;
                        }
                    }
                }

                ui.label("Window for averaging speed (ms):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.window_duration_string)
                        .desired_width(60.0)
                        .hint_text("Speed calculation window"),
                );
            });

            ui.label(format!(
                "Polling Rate: {}",
                state.events_per_second
            ));
            ui.label(format!(
                "Delta X: {}, Delta Y: {}",
                state.delta.0, state.delta.1
            ));
            ui.label(format!("Speed: {:.4} m/s", speed));
            ui.label(format!("Max Speed: {:.4} m/s", state.max_speed));

            if ui.button("Reset Max Speed").clicked() {
                state.max_speed = 0.0;
            }

            ui.separator();

            // Graphs
            ui.columns(2, |columns| {
                columns[0].label(format!("Speed Over Time"));
                Plot::new("speed_plot")
                    .allow_zoom(Vec2b::FALSE)
                    .allow_drag(Vec2b::FALSE)
                    .allow_scroll(Vec2b::FALSE)
                    .allow_double_click_reset(false)
                    .allow_boxed_zoom(false)
                    .show_grid(false)
                    .view_aspect(1.0)
                    .include_y(0.5)
                    .show(&mut columns[0], |plot_ui| {
                        let points: PlotPoints =
                            state.speed_history.iter().map(|&(x, y)| [x, y]).collect();
                        plot_ui.line(Line::new(points).fill(0.0));
                    });

                columns[1].label(format!("Polling Rate Over Time"));
                Plot::new("polling_plot")
                    .allow_zoom(Vec2b::FALSE)
                    .allow_drag(Vec2b::FALSE)
                    .allow_scroll(Vec2b::FALSE)
                    .allow_double_click_reset(false)
                    .allow_boxed_zoom(false)
                    .show_grid(false)
                    .view_aspect(1.0)
                    .include_y(1000.0)
                    .show(&mut columns[1], |plot_ui| {
                        let points: PlotPoints =
                            state.polling_history.iter().map(|&(x, y)| [x, y]).collect();
                        plot_ui.line(Line::new(points).fill(0.0));
                    });
            });
        });
    }
}

fn main() -> eframe::Result {
    // Initialize eframe
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 400.0])
            .with_title("Mouse Speed Analyser"),
        ..Default::default()
    };

    // Run eframe
    eframe::run_native(
        "Mouse Speed Analyser Analyser",
        options,
        Box::new(|_cc| Ok(Box::new(MouseApp::new()))),
    )
}