use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

#[test]
fn test_ratatui_test_backend_draw() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let size = f.area();
            assert_eq!(size, Rect::new(0, 0, 40, 10));
            let p = Paragraph::new("diffuse review").block(Block::default().borders(Borders::ALL));
            f.render_widget(p, size);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 1)].symbol(), "d");
}
