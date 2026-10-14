//! Cached synchronous isolates must safely outlive the binding's TLS cells.
#[test]
fn cached_sync_runtime_keeps_identity_and_disposes_at_real_thread_exit() {
    std::thread::spawn(|| {
        assert_eq!(
            turbo_surf_render::run_with_dom(
                "<body><p>owner</p>",
                "globalThis.held=document.querySelector('p');held.marker=67;held.textContent",
            )
            .unwrap(),
            "owner"
        );
        assert_eq!(
            turbo_surf_render::run_with_dom(
                "<body><p>owner</p>",
                "JSON.stringify([held===document.querySelector('p'),held.marker])",
            )
            .unwrap(),
            "[true,67]"
        );
    })
    .join()
    .unwrap();
}
