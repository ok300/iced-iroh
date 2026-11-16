use std::{sync::Arc, time::Duration};

use iced::{
    Task, Widget,
    alignment::Horizontal,
    clipboard,
    widget::{button, center_x, center_y, column, space, text, text_input, tooltip::Tooltip},
};
use iced_core::text::Wrapping;
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_ping::Ping;
use iroh_tickets::{Ticket, endpoint::EndpointTicket};

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application(
        || {
            (
                IcedIrohApp::default(),
                Task::perform(run_receiver(), |res| {
                    ReceiverMessage::ReceiverStarted(res.map_err(|e| e.to_string())).into()
                }),
            )
        },
        IcedIrohApp::update,
        IcedIrohApp::view,
    )
    .run()
}

#[derive(Default)]
struct IcedIrohApp {
    receiver_state: ReceiverState,
    ticket_copied: bool,
    sender_input: String,
    pong_output: String,
}

#[derive(Default)]
enum ReceiverState {
    #[default]
    Loading,
    // Keep the router alive so the receiver keeps running.
    Ready(Arc<Router>, String),
    Error(String),
}

#[derive(Clone, derive_more::From)]
enum Message {
    Sender(SenderMessage),
    Receiver(ReceiverMessage),
}

#[derive(Clone)]
enum ReceiverMessage {
    ReceiverStarted(Result<Arc<Router>, String>),
    CopyTicket,
    TicketCopied(Result<(), String>),
}

#[derive(Clone)]
enum SenderMessage {
    InputChanged(String),
    SendClicked,
    PongReceived(Result<Duration, String>),
}

impl IcedIrohApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Receiver(ReceiverMessage::ReceiverStarted(res)) => {
                self.receiver_state = match res {
                    Ok(router) => {
                        let ticket = EndpointTicket::new(router.endpoint().addr());
                        ReceiverState::Ready(router, ticket.to_string())
                    }
                    Err(err) => ReceiverState::Error(err),
                };

                Task::none()
            }

            Message::Receiver(ReceiverMessage::CopyTicket) => {
                let ticket = match &self.receiver_state {
                    ReceiverState::Ready(_, ticket) => ticket.clone(),
                    _ => String::new(),
                };

                clipboard::write(ticket).map(|result| {
                    Message::Receiver(ReceiverMessage::TicketCopied(match result {
                        Ok(()) => Ok(()),
                        Err(e) => Err(format!("{e:?}")),
                    }))
                })
            }

            Message::Receiver(ReceiverMessage::TicketCopied(res)) => {
                self.ticket_copied = res.is_ok();

                Task::none()
            }

            Message::Sender(SenderMessage::InputChanged(text)) => {
                self.sender_input = text;

                Task::none()
            }

            Message::Sender(SenderMessage::SendClicked) => {
                let ticket_str = self.sender_input.clone();

                let ticket = EndpointTicket::decode_string(&ticket_str)
                    .map_err(|e| anyhow::anyhow!("failed to parse ticket: {}", e))
                    .unwrap();

                match &self.receiver_state {
                    ReceiverState::Ready(router, _) => {
                        let send_ep = router.endpoint().clone();

                        Task::perform(run_sender(send_ep, ticket), |res| {
                            SenderMessage::PongReceived(res.map_err(|e| e.to_string())).into()
                        })
                    }
                    _ => Task::none(),
                }
            }

            Message::Sender(SenderMessage::PongReceived(res)) => {
                self.pong_output = match res {
                    Ok(rtt) => format!("ping took: {:?} to complete", rtt),
                    Err(err) => format!("err: {err}"),
                };

                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let body_receiver = match &self.receiver_state {
            ReceiverState::Ready(_, ticket) => column![
                text("My Ticket"),
                text(ticket).width(480).wrapping(Wrapping::WordOrGlyph),
                Tooltip::new(
                    button("Copy").on_press(ReceiverMessage::CopyTicket.into()),
                    text(if self.ticket_copied { "Copied" } else { "Copy" }),
                ),
            ]
            .spacing(4)
            .align_x(Horizontal::Center)
            .boxed(),
            ReceiverState::Error(err) => text(format!("error: {err}")).boxed(),
            ReceiverState::Loading => text("Starting receiver...").boxed(),
        };

        if matches!(self.receiver_state, ReceiverState::Ready(_, _)) {
            let body_sender = column![
                text_input("Enter ticket", &self.sender_input)
                    .on_input(|text| SenderMessage::InputChanged(text).into())
                    .multiline(Some(Wrapping::WordOrGlyph))
                    .width(480),
                button("Send").on_press(SenderMessage::SendClicked.into()),
                text(&self.pong_output)
            ]
            .spacing(4)
            .align_x(Horizontal::Center);

            center_y(center_x(column![body_receiver, body_sender].spacing(4)))
        } else {
            center_y(center_x(column![body_receiver]))
        }
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

async fn run_sender(own_endpoint: Endpoint, ticket: EndpointTicket) -> anyhow::Result<Duration> {
    Ping::new()
        .ping(&own_endpoint, ticket.endpoint_addr().clone())
        .await
}
