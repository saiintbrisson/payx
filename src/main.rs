use std::env;

use anyhow::{Context, Result, anyhow};
use payx::ClientBook;

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("missing input CSV file argument"))?;

    let book = ClientBook::from_csv(path)?;

    let mut writer = csv::WriterBuilder::new()
        // **NOTE:** `Decimal` does not play along nicely with `csv`s
        // serde implementation when infering the headers,
        // so I have to explicitly write them as the first record.
        .has_headers(false)
        .delimiter(b',')
        .flexible(false)
        .from_writer(std::io::stdout());

    writer.write_record(["client", "available", "held", "total", "locked"])?;

    for client in book.into_clients().values() {
        writer
            .serialize(client)
            .context("failed to write client row")?;
    }

    writer.flush().context("failed to flush writes to stdout")?;

    Ok(())
}
