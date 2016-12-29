## Replacements
This utility can be used to find and replace blocks in a minecraft world. It currently supports finding/replacing block IDs and Data, with support for arbitrary NBT data incoming. The replacements to be made are stored in a JSON file, as described below.

The basic format is made up of an object for each block ID, with sub-objects for each Data value. NBT tags can be matched by creating a match object inside of the Data object, the name of this object doesn't matter, but it must have a "fromNBT" object inside it that contains key, value pairs to search for. The match object can also have a "toNBT" object to specify NBT data changes, or additions. Block IDs and Data can be changed using "toID" and "toData" properties respectively. These can be placed directly within the Data object if NBT matching is not being used, or within a Match object if NBT matching is used. A "title" property can be used in the Data object, or the NBT match object to denote what block is being replaced

Can specify "delete": true to delete any tile entity related to this block, it is not an error if this is true and there is no tile entity. This can be specified at any level in the matching hierarchy
An array of NBT tags to delete can be specified with "deleteNBT": []
NBT tags can be added simply by including them in the "toNBT" object

