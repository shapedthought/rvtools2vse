use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about)]
pub struct Cli {
    /// RvTools File(s)
    #[clap(short, long, value_delimiter = ',', num_args = 1..)]
    pub rvtools_files: Vec<PathBuf>,

    /// Include Powered Off VMs
    #[clap(short, long, action, default_value_t = false)]
    pub include_powered_off: bool,

    /// Retention - example 30D1W1M1Y - global
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub retention: Option<String>,

    /// Output File [Optional]
    #[clap(short, long, value_parser)]
    pub output_file: Option<String>,

    /// Print converted data (VSE format)
    #[clap(short, long, action, default_value_t = false)]
    pub print: bool,

    /// Print the VM info to JSON
    #[clap(long, action, default_value_t = false)]
    pub print_json: bool,

    /// Print DC level summary
    #[clap(short, long, action, default_value_t = false)]
    pub show_info: bool,

    /// DC include list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub dc_include: Option<Vec<String>>,

    /// Cluster include list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub cluster_include: Option<Vec<String>>,

    /// DC exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub dc_exclude: Option<Vec<String>>,

    /// Cluster exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub cluster_exclude: Option<Vec<String>>,

    /// VM exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub vm_exclude: Option<Vec<String>>,

    /// Legacy mode - pre v4.1.2
    #[clap(long, action, default_value_t = false)]
    pub legacy: bool,

    /// Map DCs to a site - requires a JSON file
    #[clap(long, value_parser)]
    pub dc_site_map: Option<PathBuf>,

    /// Creates Map DC JSON template
    #[clap(long, action, default_value_t = false)]
    pub dc_site_map_template: bool,

    /// Don't use vPartition capacity
    #[clap(short, long, action, default_value_t = false)]
    pub do_not_use_vpartition: bool,

    /// Print DCs
    #[clap(long, action, default_value_t = false)]
    pub dc_print: bool,

    /// Print VM table
    #[clap(short, long, action, default_value_t = false)]
    pub vm_table_print: bool,

    /// Flatten to single site, repo and workload
    #[clap(long, action, default_value_t = false)]
    pub flatten: bool,

    /// Flatten to single cluster per-site
    #[clap(long, action, default_value_t = false)]
    pub flatten_site: bool,

    /// Plot capacity data in a bar chart
    #[clap(long, action, default_value_t = false)]
    pub plot: bool,

    /// Anonymize the data
    #[clap(long, action, default_value_t = false)]
    pub anonymize: bool,
}
