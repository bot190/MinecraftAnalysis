#!/usr/bin/env python
"""
Get unique block information for all blocks used in a world. Will eventually
be able to replace blocks. 

Should create representation of blocks which can then be printed out.
Need to do this in a way that they can be compared, and only added if unique.

"""
import os, sys
import itertools
import logging
from nbt.nbt import TAG_List, TAG_Long, TAG_Byte, TAG_Byte_Array, TAG_Int,TAG_Compound
import nbt
from multiprocessing import Pool,Queue
import threading
from collections import defaultdict
import json
from utilities import array_4bit_to_byte, array_byte_to_4bit, unpack_nbt, pack_nbt, to_json, DelayedKeyboardInterrupt
from QueueHandler import QueueHandler

# Default tags to remove, eventually make this loaded from a file
tags_to_strip = ["id", "x", "y", "z", "Items", "facing"]
replacements={}

# This function takes a flag, and tile_data dict(dict(dict))
# and flattens it back into a TAG_List
def flatten_tile_entity(tile_data):
    tile_entities = TAG_List(type=TAG_Compound)
    for _,value_x in tile_data.viewitems():
        for _,value_y in value_x.viewitems():
            while True:
                try:
                    (_,tile) = value_y.popitem()
                    tile_entities.append(tile)
                except:
                    break
    return tile_entities

# This function takes an array of block IDs and data, and creates the Add 
# bytearray if necessary, as well as converts all data to TAG_Byte_Array
def parse_block_info(blocks,data):
    add = [0] * 4096
    for i,_ in enumerate(blocks):
        if blocks[i] > 255:
            add[i] = blocks[i]//256
            blocks[i] -= add[i] * 256
    add_list = TAG_Byte_Array(name=unicode("Add"))
    add_list.value = array_byte_to_4bit(bytearray(add))
    block_list =TAG_Byte_Array(name=unicode("Blocks"))
    block_list.value = bytearray(blocks)
    data_list =TAG_Byte_Array(name=unicode("Data"))
    data_list.value = array_byte_to_4bit(bytearray(data))
    return (block_list,data_list,add_list)

def write_block_data(region_data,output_file):
    # initialize with data from first region
    world_data = region_data[0]
    for region in region_data[1:]:
        for block in region:
            try:
                world_data.index(block)
                # We've already seen this exact block
            except ValueError:
                world_data.append(block)
    out_file = open(output_file, "w+")
    out_file.write("Block ID,Data,NBT ID,NBT\n")
    for block in world_data:
        #print("Block: {0}:{1} ID: {2} Data: {3}".format(block[0],block[1], block[2], json.dumps(block[3], default=to_json)))
        out_file.write("{0};{1};{2};{3}\n".format(block[0],block[1], block[2], block[3]))
    out_file.close()

def process_block_change(title,block,data,tile,z,replace_info,chunk_modified, tile_entity_modified):
    try:
        # If toID is omitted then no change is made
        block = replace_info["toID"]
        process_region.logger.debug("Changing ID: %s to %s", title, replace_info["toID"])
        chunk_modified = True
    except:
        pass
    try:
        # If toData is omitted then no change is made  
        data = replace_info["toData"]
        process_region.logger.debug("Changing Data: %s to %s",title, replace_info["toData"])
        chunk_modified = True
    except:
        pass
   	try:
		# If toData is omitted then no change is made  
		data += replace_info["adjustData"]
		process_region.logger.debug("Adjusting Data: %s to %s",title, replace_info["adjustData"])
		chunk_modified = True
	except:
		pass
    try:
        # If delete property specified, try to remove the tile entity
        replace_info["delete"]
        if (tile.pop(z,None) != None):
            process_region.logger.debug("Deleted Tile Data: %s",title)
            chunk_modified = True
            tile_entity_modified = True
    except:
        # Block didn't have a tile entity attached
        pass
    return (block,data,tile,chunk_modified,tile_entity_modified)

def process_region(region_file):
    replacements = process_region.replacements
    region = nbt.region.RegionFile(region_file)
    # Iterate through chunks in this region file and process them
    region_data = []
    for chunk in region.iter_chunks():
        level = chunk["Level"]
        tile_data = defaultdict(lambda: defaultdict(dict))
        chunk_modified = False
        tile_entity_modified = False
        for tile_entity in level["TileEntities"]:
            x = tile_entity["x"].value
            y = tile_entity["y"].value
            z = tile_entity["z"].value
            tile_data[x][y][z] = tile_entity
        for ySec,section in enumerate(level["Sections"]):
            blocks = list(unpack_nbt(section["Blocks"]))
            data = array_4bit_to_byte(section["Data"])
            try:
                add = array_4bit_to_byte(section['Add'])
                # If add exists, then lets update the block values as necessary 
                for i,v in enumerate(add):
                    blocks[i] = (blocks[i] + 256*v)
            except (KeyError, AttributeError):
                pass
            for i,v in enumerate(blocks):
                y = i // 256
                z = (i - (y*256)) // 16
                x = (i - (y*256) - (z*16))
                y += ySec*16
                z += level["zPos"].value*16
                x += level["xPos"].value*16
                try:
                    id = tile_data[x][y][z]['id']
                    stripped_tags =unpack_nbt(tile_data[x][y][z])
                    for tag in tags_to_strip:
                        stripped_tags.pop(tag,None)
                    stripped_tags = json.dumps(stripped_tags, default=to_json)
                    block = (blocks[i], data[i], id, stripped_tags)
                except KeyError:
                    block = (blocks[i], data[i], None, None)
                try:
                    region_data.index(block)
                    # We've already seen this exact block
                except ValueError:
                    region_data.append(block)
                try:
                    # If no replacements file was passed in, don't try replacing blocks
                    replacements
                    if str(blocks[i]) in replacements:
                        # Block ID matched, lets check data
                        block_id = str(blocks[i])
                        # Match Data value, or match always if wildcard present,
                        if (str(data[i]) in replacements[block_id]):
                            block_data = str(data[i])
                        elif ("*" in replacements[block_id]):
                            block_data = unicode("*")
                        else:
                            block_data = ""
                        if block_data != "":
                            title = None
                            # Iterate through any declared NBT matches, first succesful match will be applied
                            for key,value in replacements[block_id][block_data].viewitems():
                                matched = True
                                if key == "toID" or key == "toData":
                                    break
                                elif key == "title" or key == "delete" or key == "deleteNBT":
                                    continue
                                if "fromNBT" in value and ( x in tile_data and y in tile_data[x] and z in tile_data[x][y]):
                                    # Pattern definitely given
                                    for tag,tag_data in value['fromNBT'].viewitems():
                                        # Check for tag, data pair in the tile-data
                                        if tag in tile_data[x][y][z]:
                                            if (tile_data[x][y][z][tag].value != tag_data):
                                                matched = False
                                                break
                                        else:
                                            matched = False
                                            break
                                if matched:
                                    try:
                                        title = replacements[block_id][block_data][key]["title"]
                                    except:
                                        title = key
                                    (blocks[i],data[i],tile_data[x][y],
                                    chunk_modified,tile_entity_modified) = process_block_change(
                                        title,blocks[i],data[i],tile_data[x][y],z,
                                        value,
                                        chunk_modified,tile_entity_modified)
                                    if "toNBT" in value:
                                        chunk_modified = True
                                        tile_entity_modified = True
                                        # this pattern matched, lets make the changes specified, and be on our merry way
                                        for tag,tag_data in value["toNBT"].viewitems():
                                            tile_data[x][y][z][tag] = pack_nbt(tag_data)
                                        process_region.logger.debug("Changing NBT-NBT: %s", title)
                                    if "deleteNBT" in value:
                                        for delTag in value["deleteNBT"]:
                                            try:
                                                if tile_data[x][y][z].pop(delTag,None) != None:
                                                    process_region.logger.debug("Deleted Tag %s from %s", delTag, title)
                                                    chunk_modified = True
                                                    tile_entity_modified = True
                                            except:
                                                pass
                            if not title:
                                try:
                                    title = replacements[block_id][block_data]["title"]
                                except:
                                    title = "{}:{}".format(block_id, block_data)
                            # As long as block ID and Data have matched try this, WILL override NBT matches
                            (blocks[i],data[i],tile_data[x][y],
                            chunk_modified,tile_entity_modified) = process_block_change(
                                title,blocks[i],data[i],tile_data[x][y],z,
                                replacements[block_id][block_data],
                                chunk_modified,tile_entity_modified)
                except NameError:
                    pass
                # If changes were made, update level variable for writing back to file
            if chunk_modified:
                (level["Sections"][ySec]["Blocks"],
                 level["Sections"][ySec]["Data"],
                 level["Sections"][ySec]["Add"]) = parse_block_info(blocks, data)
            # Flatten tile_data into tile_entities compound tag
            if tile_entity_modified:
                level["TileEntities"] = flatten_tile_entity(tile_data)
        try:
            replacements
            if chunk_modified:
                # Write out updated chunk
                process_region.logger.info("Writing chunk data %d,%d to %s",level["xPos"].value%32, level["zPos"].value%32, region_file)
                # Ensure that we don't interrupt a chunk write with SigInt.
                with DelayedKeyboardInterrupt():
                    region.write_chunk(level["xPos"].value%32, level["zPos"].value%32, chunk)                
        except NameError as e:
            print("Error finding: {0}".format(e))
            pass
        try:
            del tile_entities
            del level
        except:
            pass
    return region_data

def process_init(q,Replace):
    process_region.replacements = Replace
    process_region.qh = QueueHandler(q)
    process_region.logger = logging.getLogger(__name__)
    process_region.logger.setLevel(logging.DEBUG)
    process_region.logger.addHandler(process_region.qh)
    
def logger_thread(q):
    logger = logging.getLogger(__name__)
    while True:
        record = q.get()
        if record is None:
            break
        logger.handle(record)

def configure_logging():
    logger = logging.getLogger(__name__)
    logger.setLevel(logging.DEBUG)
    fh = logging.FileHandler(os.path.join(os.getcwd(), "replacements.log"))
    formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
    fh.setFormatter(formatter)
    fh.setLevel(logging.DEBUG)
    ch = logging.StreamHandler()
    ch.setFormatter(formatter)
    ch.setLevel(logging.INFO)
    logger.addHandler(fh)
    logger.addHandler(ch)
    return logger
    
def main(world_folder, replacement_file_name):
    global replacements
    world = nbt.world.WorldFolder(world_folder)
    logger = configure_logging()
    logger.info("Starting processing of %s", world_folder)
    if not isinstance(world, nbt.world.AnvilWorldFolder):
        logger.error("%s is not an Anvil world" % (world_folder))
        return 65 # EX_DATAERR
    if replacement_file_name != None:
        logger.info("Using Replacements file: %s", replacement_file_name)
        with open(replacement_file_name, 'r') as replacement_file:
            replacements = json.load(replacement_file)
    # get list of region files, going to pass this into function to process region
    region_files = world.get_regionfiles()
    
    # Parallel
    q = Queue()
    lp = threading.Thread(target=logger_thread, args=[q])
    lp.start()
    p = Pool(processes=4,initializer=process_init, initargs=[q,replacements], maxtasksperchild=1)
    region_data = p.map(process_region, region_files)
    # Map has finished up, lets close the logging QUEUE
    q.put(None)
    lp.join()
    
    # Not Parallel
#     region_data = map(process_region, region_files)
    
    # Write output data
    write_block_data(region_data,"output.txt")
    return 0

def usage(message=None, appname=None):
    if appname == None:
        appname = os.path.basename(sys.argv[0])
    print("Usage: %s WORLD_FOLDER [REPLACEMENT_FILE]" % appname)
    if message:
        print("%s: error: %s" % (appname, message))

if __name__ == '__main__':
    if (len(sys.argv) < 2) or (len(sys.argv) > 3):
        usage()
        sys.exit(64) # EX_USAGE
    world_folder = sys.argv[1]
    if (len(sys.argv) == 3):
        replacement_file_name = sys.argv[2]
        if (not os.path.exists(replacement_file_name)):
            usage("Replacements file ({}) does not exist".format(world_folder))
            sys.exit(72) # EX_IOERR
    else:
        replacement_file_name = None
    
    # clean path name, eliminate trailing slashes:
    world_folder = os.path.normpath(world_folder)
    if (not os.path.exists(world_folder)):
        usage("No such folder as "+world_folder)
        sys.exit(72) # EX_IOERR
    
    sys.exit(main(world_folder, replacement_file_name))
