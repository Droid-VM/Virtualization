# Tests

## Test data

The test data are generated from manually written `.dts` files. You can use the `dtc` tool to extract and decompile the `.dtb` files into human-readable `.dts` file with the following command

```bash
dtc -I dtb -O dts -o test_tree1.dts test_tree1.dtb
```