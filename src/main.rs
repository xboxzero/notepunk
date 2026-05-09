use leptos::mount::mount_to_body;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <main class="page">
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · remix · remember //"</p>
            </header>
            <section class="placeholder">
                <p>"phase one : scaffold standing."</p>
                <p class="dim">
                    "rust + wasm + cytoscape + whisper // the beat goes on"
                </p>
            </section>
        </main>
    }
}
