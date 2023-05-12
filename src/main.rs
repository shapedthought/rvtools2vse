use office::{Excel, DataType, Range};
use anyhow::Result;
use itertools::Itertools;

fn get_values(workbook: &Range) -> f64 {

    let mut total_cap: Vec<f64> = Vec::new();

    for row in workbook.rows() {
      match &row[1] {
          DataType::String(t) => {
              if t.contains("poweredOn") {
                 match &row[36] {
                    DataType::Float(t) => total_cap.push(t.to_owned()),
                    DataType::Int(t) => total_cap.push(t.to_owned() as f64),
                    _ => continue
                 }
            }
              }
          _ => panic!("ahhhh")
          }
      }
 
    let total_sum: f64 = total_cap.iter().sum();

    total_sum
 
}

#[derive(Debug, Clone)]
struct Vinfo {
    vm_name: String,
    datacenter: String,
    cluster: String,
    capacity: f64
}

#[derive(Debug, Clone)]
struct Vpartition {
    vm_name: String,
    capacity: f64
}

#[derive(Debug, Clone)]
struct Datacenter {
    name: String,
    cluster: String,
    capacity: f64
}

fn main() -> Result<()> {
  let mut excel = Excel::open("rvtools2.xlsx").unwrap();

  let workbook = excel.worksheet_range("vInfo").unwrap();

  let partition = excel.worksheet_range("vPartition").unwrap();

  let mut info_vec: Vec<Vinfo> = Vec::new();

  for row in workbook.rows() {
      if let DataType::String(vm_name) = &row[0] {
          if let DataType::Float(capacity) = row[36] {
              if let DataType::String(datacenter) = &row[59] {
                if let DataType::String(cluster) = &row[60] {
                   info_vec.push(Vinfo {
                       vm_name: vm_name.to_string(),
                       datacenter: datacenter.to_string(),
                       cluster: cluster.to_string(),
                       capacity
                   })
                }
            }
          }
      }
  }

  let mut part_vec: Vec<Vpartition> = Vec::new();

  for row in partition.rows() {
      if let DataType::String(vm_name) = &row[0] {
             if let DataType::Float(capacity) = row[5] {
                part_vec.push(Vpartition {
                    vm_name: vm_name.to_string(),
                    capacity
                })
         }
      }
  }

  let total_sum = get_values(&workbook);

  let devisor = 1024_f64.powf(2.0);
  
  println!("{:.2?} TB", total_sum / devisor);

  let grouped: Vec<Vpartition> = part_vec.into_iter()
      .sorted_by_key(|x| x.vm_name.clone())
      .group_by(|x|x.vm_name.clone())
      .into_iter()
      .map(|(name, group)| {
          let total = group.map(|x| x.capacity).sum();
          Vpartition {
              vm_name: name,
              capacity: total }
      }).collect();

  let mut combined: Vec<Vinfo> = Vec::new();

  for i in &info_vec {
      let mut found_match = false;
      for j in &grouped {
          if i.vm_name == j.vm_name {
              let low_cap = f64::min(i.capacity, j.capacity);
              let new_st = Vinfo {
                  vm_name: i.vm_name.clone(),
                  datacenter: i.datacenter.clone(),
                  cluster: i.cluster.clone(),
                  capacity: low_cap
              };
              combined.push(new_st);
              found_match = true;
          } 
      }
      if !found_match {
          combined.push(i.clone())
      }
  } 

  let mut datacenters: Vec<Datacenter> = Vec::new();

  combined.iter()
      .sorted_by_key(|s| (&s.datacenter, &s.cluster))
      .group_by(|s| (&s.datacenter, &s.cluster))
      .into_iter()
      .for_each(|(key, group)| {
          let cap: f64 = group.map(|x| x.capacity).collect::<Vec<_>>().iter().sum();
          datacenters.push(
              Datacenter {
                name: key.0.to_string(),
                cluster: key.1.to_string(),
                capacity: cap / devisor
              }
              )
      });

  println!("{:#?}", datacenters);

  println!("vinfo length: {:?}, combined length: {:?}", info_vec.len(), combined.len());

  Ok(())
}
