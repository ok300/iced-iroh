use std::sync::Arc;

use iced::clipboard;
use iced::widget::{center_x, center_y};
use iced::{
    Task, Widget,
    alignment::Horizontal,
    widget::{button, column, text, tooltip::Tooltip},
};
use iced_core::text::Wrapping;
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_ping::Ping;
use iroh_tickets::endpoint::EndpointTicket;

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    let app = iced::application(
        || {
            let task = Task::perform(run_receiver(), |res| {
                Message::ReceiverStarted(res.map_err(|e| e.to_string()))
            });

            (IcedIrohApp::default(), task)
        },
        IcedIrohApp::update,
        IcedIrohApp::view,
    );

    app.run()
}

#[derive(Default)]
struct IcedIrohApp {
    state: State,
    ticket_copied: bool,
}

#[derive(Default)]
enum State {
    #[default]
    Loading,
    // Keep the router alive so the receiver keeps running; only its ticket string is shown.
    #[allow(dead_code)]
    Ready(Arc<Router>, String),
    Error(String),
}

#[derive(Clone)]
enum Message {
    ReceiverStarted(Result<Arc<Router>, String>),
    CopyTicket,
    TicketCopied(Result<(), String>),
}

impl IcedIrohApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ReceiverStarted(res) => {
                match res {
                    Ok(router) => {
                        let ticket = EndpointTicket::new(router.endpoint().addr());
                        self.state = State::Ready(router.clone(), format!("{ticket}"));
                    }
                    Err(err) => self.state = State::Error(err),
                }

                Task::none()
            }
            Message::CopyTicket => {
                let ticket = match &self.state {
                    State::Ready(_, ticket) => ticket.clone(),
                    _ => String::new(),
                };

                clipboard::write(ticket).map(|result| {
                    Message::TicketCopied(match result {
                        Ok(()) => Ok(()),
                        Err(e) => Err(format!("{e:?}")),
                    })
                })
            }
            Message::TicketCopied(res) => {
                self.ticket_copied = res.is_ok();

                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let body = match &self.state {
            State::Ready(_, ticket) => {
                let copy_button = button("Copy").on_press(Message::CopyTicket);

                let tooltip = match self.ticket_copied {
                    true => Tooltip::new(copy_button, text("Copied")),
                    false => Tooltip::new(copy_button, text("Copy")),
                };

                column![
                    text("My Ticket"),
                    text(ticket)
                        .width(480)
                        .height(96)
                        .wrapping(Wrapping::WordOrGlyph),
                    tooltip
                ]
                .spacing(4)
                .align_x(Horizontal::Center)
                .boxed()
            }
            State::Error(err) => text(format!("error: {err}")).boxed(),
            State::Loading => text("Starting receiver...").boxed(),
        };

        center_y(center_x(body))
    }
}

async fn run_receiver() -> anyhow::Result<Arc<Router>> {
    let endpoint = Endpoint::bind(presets::N0).await?;
    endpoint.online().await;

    let ping = Ping::new();
    let router = Router::builder(endpoint)
        .accept(iroh_ping::ALPN, ping)
        .spawn();

    Ok(Arc::new(router))
}
