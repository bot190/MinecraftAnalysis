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

# local module
try:
	import nbt
except ImportError:
# nbt not in search path. Let's see if it can be found in the parent folder
	extrasearchpath = os.path.realpath(os.path.join(__file__,os.pardir,os.pardir))
	if not os.path.exists(os.path.join(extrasearchpath,'nbt')):
		raise
	sys.path.append(extrasearchpath)
from multiprocessing import Pool,Queue
import threading
from collections import defaultdict
import json
from utilities import array_4bit_to_byte, array_byte_to_4bit, unpack_nbt, pack_nbt, to_json, DelayedKeyboardInterrupt
from QueueHandler import QueueHandler

# Default tags to remove, eventually make this loaded from a file
tags_to_strip = ["id", "x", "y", "z", "Items", "facing"]
replacements={}

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
			# defaultdict(lambda: defaultdict(dict)) was supposed to handle 
			# this, but it causes some major weirdness, so doing it by hand
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
									try:
										# If toID is omitted then no change is made
										blocks[i] = value["toID"]
										process_region.logger.debug("Changing NBT-ID: %s", title)
										chunk_modified = True
									except:
										pass
									try:
										# If toData is omitted then no change is made  
										data[i] = value["toData"]
										process_region.logger.debug("Changing NBT-Data: %s", title)
										chunk_modified = True
									except:
										pass
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
									elif "delete" in value:
										try:
											# If delete property specified, try to remove the tile entity
											if (tile_data[x][y].pop(z,None) != None):
												process_region.logger.debug("Deleted Tile Data: %s",title)
												chunk_modified = True
												tile_entity_modified = True
										except:
											# Block didn't have a tile entity attached
											pass
							if not title:
								try:
									title = replacements[block_id][block_data]["title"]
								except:
									title = "{}:{}".format(block_id, block_data)
							# As long as block ID and Data have matched try this, WILL override NBT matches
							try:
								# If toID is omitted then no change is made
								blocks[i] = replacements[block_id][block_data]["toID"]
								process_region.logger.debug("Changing ID: %s", title)
								chunk_modified = True
							except:
								pass
							try:
								# If toData is omitted then no change is made  
								data[i] = replacements[block_id][block_data]["toData"]
								process_region.logger.debug("Changing Data: %s",title)
								chunk_modified = True
							except:
								pass
							try:
								# If delete property specified, try to remove the tile entity
								replacements[block_id][block_data]["delete"]
								tile_data[x][y].pop(z,None)
								process_region.logger.debug("Deleted Tile Data: %s",title)
								tile_entity_modified = True
								chunk_modified = True
							except:
								# Block didn't have a tile entity attached
								pass
				except NameError:
					pass
				# If changes were made, update level variable for writing back to file
			if chunk_modified:
				add = [0] * 4096
				for i,v in enumerate(blocks):
					if blocks[i] > 255:
						add[i] = blocks[i]//256
						blocks[i] -= add[i] * 256
				add_list = TAG_Byte_Array(name=unicode("Add"))
				add_list.value = array_byte_to_4bit(bytearray(add))
				level["Sections"][ySec]["Add"] = add_list
				block_list =TAG_Byte_Array(name=unicode("Blocks"))
				block_list.value = bytearray(blocks)
				level["Sections"][ySec]["Blocks"] = block_list
				data_list =TAG_Byte_Array(name=unicode("Data"))
				data_list.value = array_byte_to_4bit(bytearray(data))
				level["Sections"][ySec]["Data"] = data_list
		if tile_entity_modified:
			#flatten tile_data back into a TAG_List
			tile_entities = TAG_List(type=TAG_Compound)
			for x,value_x in tile_data.viewitems():
				for y,value_y in value_x.viewitems():
					for z,tile in value_y.viewitems():
						tile_entities.append(tile)
			level["TileEntities"] = tile_entities
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

def main(world_folder, replacement_file_name):
	global replacements
	world = nbt.world.WorldFolder(world_folder)
	logger.info("Starting processing of %s", world_folder)
	if not isinstance(world, nbt.world.AnvilWorldFolder):
		logger.error("%s is not an Anvil world" % (world_folder))
		return 65 # EX_DATAERR
	if replacement_file_name != None:
		logger.info("Using Replacements file: %s", replacement_file_name)
		with open(replacement_file_name, 'r') as replacement_file:
			replacements = json.load(replacement_file)
	# get list of region files, going to pass this into function to process region
	region_files = world.get_regionfiles();
	
	
	# Parallel
	q = Queue()
	lp = threading.Thread(target=logger_thread, args=[q])
	lp.start()
	p = Pool(processes=1, initializer=process_init, initargs=[q,replacements])
	region_data = p.map(process_region, region_files)
	
	# Not Parallel
# 	region_data = map(process_region, region_files)
	
	# initialize with data from first region
	world_data = region_data[0]
	for region in region_data[1:]:
		for block in region:
			try:
				world_data.index(block)
				# We've already seen this exact block
			except ValueError:
				world_data.append(block)
	out_file = open("output.txt", "w+")
	out_file.write("Block ID,Data,NBT ID,NBT\n")
	for block in world_data:
		#print("Block: {0}:{1} ID: {2} Data: {3}".format(block[0],block[1], block[2], json.dumps(block[3], default=to_json)))
		out_file.write("{0};{1};{2};{3}\n".format(block[0],block[1], block[2], block[3]))
	out_file.close()
	q.put(None)
	lp.join()
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
