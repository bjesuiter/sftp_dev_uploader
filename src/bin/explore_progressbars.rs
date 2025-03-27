use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rand::Rng;
use std::{thread, time::Duration};

fn main() {
    // Experiment 1: One progressbar
    // ex1_draw_1_progressbar();

    // Experiment 2: Spinner
    // ex2_draw_spinner();

    // Experiment 3: Multi progress bars
    ex3_multi();

    // let progress_bars = MultiProgress::new();
    // (0..6).for_each(|_| {
    //     let pb = progress_bars.add(ProgressBar::new(100));
    //     let style = ProgressStyle::default_bar().template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})");

    //     match style {
    //         Ok(style) => pb.set_style(style),
    //         Err(_) =>{
    //             panic!("DEV ERROR: Failed to set progress bar style, broken template!");
    //         },
    //     }
    //     progress_bars.add(pb);

    // });

    // thread::spawn(|| {
    //     for i in 1..6 {
    //         println!("hi number {i} from the spawned thread!");
    //         thread::sleep(Duration::from_millis(1));
    //     }
    // });
}

fn ex1_draw_1_progressbar() {
    use indicatif::ProgressBar;

    let bar = ProgressBar::new(1000);
    for _ in 0..1000 {
        bar.inc(1);
        thread::sleep(Duration::from_millis(1));
    }
    bar.finish();
}

fn ex2_draw_spinner() {
    let bar = ProgressBar::new_spinner();
    bar.enable_steady_tick(Duration::from_millis(100));
    thread::sleep(Duration::from_millis(1000));
    bar.finish();
}

/**
 * Multi progress bars
 * Src: https://github.com/console-rs/indicatif/blob/main/examples/multi.rs
 */
fn ex3_multi() {
    let m = MultiProgress::new();
    let sty = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    let n = 200;
    let pb = m.add(ProgressBar::new(n));
    pb.set_style(sty.clone());
    pb.set_message("todo");
    let pb2 = m.add(ProgressBar::new(n));
    pb2.set_style(sty.clone());
    pb2.set_message("finished");

    let pb3 = m.insert_after(&pb2, ProgressBar::new(1024));
    pb3.set_style(sty);

    m.println("starting!").unwrap();

    let mut threads = vec![];

    let m_clone = m.clone();
    let h3 = thread::spawn(move || {
        for i in 0..1024 {
            thread::sleep(Duration::from_millis(2));
            pb3.set_message(format!("item #{}", i + 1));
            pb3.inc(1);
        }
        m_clone.println("pb3 is done!").unwrap();
        pb3.finish_with_message("done");
    });

    for i in 0..n {
        thread::sleep(Duration::from_millis(15));
        if i == n / 3 {
            thread::sleep(Duration::from_secs(2));
        }
        pb.inc(1);
        let m = m.clone();
        let pb2 = pb2.clone();
        threads.push(thread::spawn(move || {
            let spinner = m.add(ProgressBar::new_spinner().with_message(i.to_string()));
            spinner.enable_steady_tick(Duration::from_millis(100));
            thread::sleep(
                rand::thread_rng().gen_range(Duration::from_secs(1)..Duration::from_secs(5)),
            );
            pb2.inc(1);
        }));
    }
    pb.finish_with_message("all jobs started");

    for thread in threads {
        let _ = thread.join();
    }
    let _ = h3.join();
    pb2.finish_with_message("all jobs done");
    m.clear().unwrap();
}
