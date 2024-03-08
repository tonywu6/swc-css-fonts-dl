# swc-css-fonts-dl

Download remote fonts referenced in stylesheets. Powered by SWC.

```bash
cargo install swc-css-fonts-dl --git https://github.com/tonywu6/swc-css-fonts-dl
```

```
Usage: swc-css-fonts-dl [OPTIONS]

Options:
  -c, --config <FILE>              Path to a YAML config file; defaults to .swc-css-fonts-dl.config.yaml in the working directory
      --concurrency <CONCURRENCY>  Maximum number of concurrent downloads [default: 10]
  -h, --help                       Print help
```

```yaml
out-dir: ./dist
sources:
  - from: https://rsms.me/inter/inter.css
    into: inter.css
  - from: https://fonts.googleapis.com/css2?family=Noto+Sans+SC:wght@100;300;400;500;600;700;900&display=swap
    into: noto-sans-sc+variable.css
    # Google Fonts serves different stylesheets based on User-Agent capabilities
    user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36
```
