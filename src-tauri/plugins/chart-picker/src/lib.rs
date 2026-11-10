use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
pub struct ChartPicker<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(target_os = "android")]
impl<R: Runtime> ChartPicker<R> {
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
            Ok(())
        })
        .build()
}
