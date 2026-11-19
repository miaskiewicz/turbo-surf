use turbo_surf_render::render_hydrate;

#[tokio::test]
async fn repeated_style_discovery_keeps_insertions_and_text_replacements_live() {
    let html = render_hydrate(
        r#"<head><style>.target{color:rgb(1,2,3)}</style></head><body><div class=target>ok</div><script>
        let last;
        for(let i=0;i<20;i++){last=document.createElement('style');document.head.append(last);last.textContent='.target{color:rgb('+i+',0,0)}';}
        last.textContent='.target{color:rgb(4,5,6)}';
        document.body.dataset.color=getComputedStyle(document.querySelector('.target')).color;
        </script></body>"#,
        "https://example.test/",
    )
    .await
    .unwrap();
    assert!(html.contains("data-color=\"rgb(4, 5, 6)\""), "{html}");
}
