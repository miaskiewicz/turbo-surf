#[cfg(test)]
mod tests {
    use turbo_dom_parser::rtdom::Tree;

    const HTML: &str = r#"<!doctype html><title>Packaged consumer</title>
        <body><main id="root">Before</main><a href="/next">Next</a></body>"#;
    const BASE: &str = "https://example.test/docs/";

    #[test]
    fn published_crates_share_types_dom_runtime_and_raster_hooks() {
        assert_eq!(turbo_surf_core::VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(turbo_surf_mcp::VERSION, turbo_surf_core::VERSION);

        let navigation: turbo_surf_core::crawl::Nav = turbo_surf_page::parse_nav(HTML, BASE, 200);
        assert_eq!(navigation.title, "Packaged consumer");
        assert_eq!(navigation.status, 200);
        assert_eq!(navigation.links, ["https://example.test/next"]);
        assert!(navigation.error.is_none());

        let tree = Tree::parse(HTML);
        assert!(turbo_surf_view::text(&tree, tree.root()).contains("Before"));
        let _: &dyn turbo_surf_core::challenge::PowEngine = &turbo_surf_render::V8PowEngine;
        assert!(matches!(
            turbo_surf_render::PageSessionOptions::default().window_timer_mode,
            turbo_surf_render::WindowTimerMode::Virtual
        ));
        turbo_surf_render::ensure_platform();
        assert_eq!(turbo_surf_render::v8_version(), "15.0.245.2-rusty");

        let pixels = turbo_surf_raster::canvas_ops_rgba(
            1,
            1,
            r##"[["fillRect", [0, 0, 1, 1], "#ff0000"]]"##,
        )
        .expect("packaged software rasterizer");
        assert_eq!(pixels, [255, 0, 0, 255]);

        // This installs the actual MCP/raster bridge without solver or .env setup.
        turbo_surf_mcp::initialize_render_hooks();
        let result = turbo_surf_render::run_with_dom(
            HTML,
            r#"(() => {
                const answer = 6 * 7;
                const root = document.getElementById('root');
                root.textContent = 'Packaged ' + answer;
                const canvas = document.createElement('canvas');
                canvas.width = 1; canvas.height = 1;
                const context = canvas.getContext('2d');
                context.fillStyle = '#ff0000';
                context.fillRect(0, 0, 1, 1);
                return JSON.stringify({
                    answer,
                    text: root.textContent,
                    pixels: Array.from(context.getImageData(0, 0, 1, 1).data)
                });
            })()"#,
        )
        .expect("packaged V8/DOM runtime");
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["answer"], 42);
        assert_eq!(result["text"], "Packaged 42");
        assert_eq!(result["pixels"], serde_json::json!(pixels));
    }
}
