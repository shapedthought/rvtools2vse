use termplot::*;

pub fn plot_data(data: Vec<f64>, site_names: Vec<String>) {
    let mut plot = Plot::default();

    let item_count = data.len() as f64;
    let max_value = data
        .iter()
        .max_by(|x, y| x.partial_cmp(y).unwrap())
        .unwrap();
    plot.set_domain(Domain(0.0..item_count))
        .set_codomain(Domain(0.0..*max_value))
        .set_title("Capacity")
        .set_x_label(format!("{}", site_names.join(", ")).as_str())
        .set_y_label("y: Capacity (TB)")
        .set_size(Size::new(150, 75))
        .add_plot(Box::new(plot::Bars::new(data)));

    println!("Note that sites with less the 1TB are filtered out.");
    println!("{plot}");
}
