use anyhow::{Error, Result};
use tokio::{stream::StreamExt, sync::mpsc};
use tokio_i3ipc::{
    event::{Event, Subscribe, WindowChange},
    msg::Msg,
    reply::{Node, NodeLayout},
    I3,
};

fn split(n: &Node) -> &'static str {
    if n.name == Some("Alacritty".to_string()) || n.name == Some("xterm".to_string()) {
        // right stack split if we are dealing with a termial
        // TODO(ethan): this doesn't handle splitting the left window
        //              correctly after the first time. I really want
        //              it to add a new window to the stack on the
        //              right.
        if n.rect.x == 0 {
            "split h"
        } else {
            "split v"
        }
    } else {
        // spiral split if we are not dealing with a termial
        if n.window_rect.width > n.window_rect.height {
            "split h"
        } else {
            "split v"
        }
    }
}

// walk the tree and determine if `window_id` has tabbed parent
fn has_tabbed_parent(node: &Node, window_id: usize, tabbed: bool) -> bool {
    if node.id == window_id {
        tabbed
    } else {
        node.nodes.iter().any(|child| {
            has_tabbed_parent(
                child,
                window_id,
                matches!(node.layout, NodeLayout::Tabbed | NodeLayout::Stacked),
            )
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    flexi_logger::Logger::with_env().start()?;
    let (mut send, mut recv) = mpsc::channel::<&'static str>(10);

    let s_handle = tokio::spawn(async move {
        let mut event_listener = {
            let mut i3 = I3::connect().await?;
            i3.subscribe([Subscribe::Window]).await?;
            i3.listen()
        };

        let i3 = &mut I3::connect().await?;

        while let Some(Ok(Event::Window(window_data))) = event_listener.next().await {
            if WindowChange::Focus == window_data.change {
                let is_tabbed = matches!(
                    window_data.container.layout,
                    NodeLayout::Tabbed | NodeLayout::Stacked
                );

                let root = i3.get_tree().await?;
                let (name, tabbed_parent) = (
                    &window_data.container.name,
                    has_tabbed_parent(&root, window_data.container.id, is_tabbed),
                );
                log::debug!("name={:?}, tabbed_parent={}", name, tabbed_parent);

                if !tabbed_parent {
                    send.send(split(&window_data.container)).await?;
                }
            }
        }
        log::debug!("Sender loop ended");
        Ok::<_, Error>(())
    });

    let r_handle = tokio::spawn(async move {
        let mut i3 = I3::connect().await?;
        while let Some(cmd) = recv.recv().await {
            i3.send_msg_body(Msg::RunCommand, cmd).await?;
        }
        log::debug!("Receiver loop ended");
        Ok::<_, Error>(())
    });

    let (send, recv) = tokio::try_join!(s_handle, r_handle)?;
    send.and(recv)?;
    Ok(())
}
