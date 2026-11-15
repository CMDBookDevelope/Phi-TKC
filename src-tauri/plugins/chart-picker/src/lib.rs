use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_chart_picker);

#[cfg(any(target_os = "android", target_os = "ios"))]
pub struct ChartPicker<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(any(target_os = "android", target_os = "ios"))]
impl<R: Runtime> ChartPicker<R> {
    pub fn preview(
        &self,
        session: u64,
        width: u32,
        height: u32,
        show_fps: bool,
    ) -> Result<(), tauri::plugin::mobile::PluginInvokeError> {
        self.0.run_mobile_plugin::<serde_json::Value>("preview", serde_json::json!({ "session": session, "width": width, "height": height, "showFps": show_fps }))
            .map(|_| ())
    }

    #[cfg(target_os = "ios")]
    pub fn save_video(
        &self,
        path: &str,
    ) -> Result<PhotoSave, tauri::plugin::mobile::PluginInvokeError> {
        self.0
            .run_mobile_plugin("saveVideo", serde_json::json!({ "path": path }))
    }
    #[cfg(target_os = "android")]
    pub fn assets(&self) -> Result<String, tauri::plugin::mobile::PluginInvokeError> {
        #[derive(serde::Deserialize)]
        struct Response {
            path: String,
        }
        self.0
            .run_mobile_plugin::<Response>("prepareAssets", serde_json::json!({}))
            .map(|response| response.path)
    }

    pub fn pick(&self) -> Result<Option<String>, tauri::plugin::mobile::PluginInvokeError> {
        #[derive(serde::Deserialize)]
        struct Response {
            path: Option<String>,
        }
        self.0
            .run_mobile_plugin::<Response>("pick", serde_json::json!({}))
            .map(|response| response.path)
    }

    pub fn pick_folder(&self) -> Result<Option<String>, tauri::plugin::mobile::PluginInvokeError> {
        #[derive(serde::Deserialize)]
        struct Response {
            path: Option<String>,
        }
        self.0
            .run_mobile_plugin::<Response>("pickFolder", serde_json::json!({}))
            .map(|response| response.path)
    }
}

#[cfg(target_os = "ios")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoSave {
    pub saved: bool,
    pub asset_id: Option<String>,
    pub message: Option<String>,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("chart-picker")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle =
                    _api.register_android_plugin("com.cnmdbook.chartpicker", "ChartPickerPlugin")?;
                _app.manage(ChartPicker(handle));
            }
            #[cfg(target_os = "ios")]
            {
                use tauri::Manager;
                let handle = _api.register_ios_plugin(init_plugin_chart_picker)?;
                _app.manage(ChartPicker(handle));
            }
            Ok(())
        })
        .build()
}
