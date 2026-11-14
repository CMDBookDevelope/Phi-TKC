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
    #[cfg(target_os = "android")]
    pub fn assets(&self) -> Result<String, tauri::plugin::mobile::PluginInvokeError> {
        #[derive(serde::Deserialize)]
        struct Response { path: String }
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
