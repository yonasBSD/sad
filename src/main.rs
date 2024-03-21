#![deny(clippy::all, clippy::cargo, clippy::nursery, clippy::pedantic)]
#![allow(
  clippy::cargo_common_metadata,
  clippy::multiple_crate_versions,
  clippy::wildcard_dependencies
)]

mod argparse;
mod displace;
mod fs_pipe;
mod fzf;
mod input;
mod output;
mod subprocess;
mod types;
mod udiff;
mod udiff_spec;

use {
  ansi_term::Colour,
  argparse::{parse_args, parse_opts, Options},
  async_channel::Receiver as MPMCR,
  displace::displace,
  futures::{
    future::{select, try_join3, try_join_all, Either},
    pin_mut,
  },
  input::{stream_in, Payload},
  output::stream_out,
  std::{
    convert::Into,
    ffi::OsString,
    process::{ExitCode, Termination},
    sync::Arc,
    thread::available_parallelism,
  },
  tokio::{
    runtime::Builder,
    sync::mpsc::{self, Receiver},
    task::{spawn, JoinHandle},
  },
  types::{Abort, Fail},
};

fn stream_trans(
  abort: &Arc<Abort>,
  threads: usize,
  opts: &Options,
  stream: &MPMCR<Payload>,
) -> (JoinHandle<()>, Receiver<OsString>) {
  let a_opts = Arc::new(opts.clone());
  let (tx, rx) = mpsc::channel::<OsString>(1);

  let handles = (1..=threads * 2)
    .map(|_| {
      let abort = abort.clone();
      let stream = stream.clone();
      let opts = a_opts.clone();
      let tx = tx.clone();

      spawn(async move {
        loop {
          let f1 = abort.notified();
          let f2 = stream.recv();
          pin_mut!(f1);
          pin_mut!(f2);

          match select(f1, f2).await {
            Either::Left(_) | Either::Right((Err(_), _)) => break,
            Either::Right((Ok(payload), _)) => match displace(&opts, payload).await {
              Ok(displaced) => {
                if tx.send(displaced).await.is_err() {
                  break;
                }
              }
              Err(err) => {
                abort.send(err).await;
                break;
              }
            },
          }
        }
      })
    })
    .collect::<Vec<_>>();

  let abort = abort.clone();
  let handle = spawn(async move {
    if let Err(err) = try_join_all(handles).await {
      abort.send(err.into()).await;
    }
  });
  (handle, rx)
}

async fn run(abort: &Arc<Abort>, threads: usize) -> Result<(), Fail> {
  //let (mode, args) = parse_args();
  //let (h_1, input_stream) = stream_in(abort, &mode, &args);
  //let opts = parse_opts(mode, args)?;
  //let (h_2, trans_stream) = stream_trans(abort, threads, &opts, &input_stream);
  //let h_3 = stream_out(abort, &opts, trans_stream);
  //try_join3(h_1, h_2, h_3).await?;
  Ok(())
}

fn main() -> impl Termination {
  let threads = available_parallelism().map(Into::into).unwrap_or(6);
  let rt = Builder::new_multi_thread()
    .enable_io()
    .max_blocking_threads(threads)
    .build()
    .expect("runtime failure");

  let errors = rt.block_on(async {
    let abort = Abort::new();
    if let Err(err) = run(&abort, threads).await {
      let mut errs = abort.fin().await;
      errs.push(err);
      errs
    } else {
      abort.fin().await
    }
  });

  match errors[..] {
    [] => ExitCode::SUCCESS,
    [Fail::Interrupt] => ExitCode::from(130),
    _ => {
      for err in errors {
        eprintln!("{}", Colour::Red.paint(format!("{err}")));
      }
      ExitCode::FAILURE
    }
  }
}
