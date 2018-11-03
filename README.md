# devsync

[dd](https://en.wikipedia.org/wiki/Dd_(Unix)) like application trying to avoid write of blocks with same data in source and destionation file.
No fancy block hashing, just compare block by block and write on differ.
Hence output file needs to be seekable.

### Example usage
`$ unxz -c some_file.xz | devsync -b 32KiB -u 8 --dry-run some_block_device`
