## Replacements
This utility can be used to find and replace blocks in a minecraft world. It currently supports finding/replacing block IDs and Data, with support for arbitrary NBT data incoming. The replacements to be made are stored in a JSON file, as described below.

Can specify "delete": true to delete any tile entity related to this block, it is not an error if this is true and there is no tile entity. This can be specified at any level in the matching hierarchy
An array of NBT tags to delete can be specified with "deleteNBT": []
NBT tags can be added simply by including them in the "toNBT" object