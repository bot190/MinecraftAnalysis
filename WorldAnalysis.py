#!/usr/bin/env python
"""
Get unique block information for all blocks used in a world. Will eventually
be able to replace blocks. 

Should create representation of blocks which can then be printed out.
Need to do this in a way that they can be compared, and only added if unique.

"""
import os, sys
import itertools

# local module
try:
	import nbt
except ImportError:
# nbt not in search path. Let's see if it can be found in the parent folder
	extrasearchpath = os.path.realpath(os.path.join(__file__,os.pardir,os.pardir))
	if not os.path.exists(os.path.join(extrasearchpath,'nbt')):
		raise
	sys.path.append(extrasearchpath)
from multiprocessing import Pool
from collections import defaultdict
import json
from utilities import array_4bit_to_byte, array_byte_to_4bit, unpack_nbt, pack_nbt, to_json

# Default tags to remove, eventually make this loaded from a file
tags_to_strip = ["id", "x", "y", "z", "Items"]
replacements={}

def process_region(region_file):
	region = nbt.region.RegionFile(region_file)
	# Iterate through chunks in this region file and process them
	region_data = []
	for chunk in region.iter_chunks():
		level = unpack_nbt(chunk["Level"])
		tile_data = defaultdict(lambda: defaultdict(dict))
		chunk_modified = False
		for tile_entity in level["TileEntities"]:
			try:
				tile_data[tile_entity["x"]][tile_entity["y"]][tile_entity["z"]] = (tile_entity["id"], tile_entity)
			except KeyError:
				pass
		for ySec,section in enumerate(level["Sections"]):
			blocks = list(section["Blocks"])
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
				z += level["zPos"]*16
				x += level["xPos"]*16
				try:
					id = tile_data[x][y][z][0]
					stripped_tags = tile_data[x][y][z][1]
					for tag in tags_to_strip:
						stripped_tags.pop(tag,None)
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
						if str(data[i]) in replacements[block_id]:
							chunk_modified = True
							block_data = str(data[i])
							# todo: Data matched, check any NBT if it exists
							blocks[i] = replacements[block_id][block_data]["toID"]
							data[i] = replacements[block_id][block_data]["toData"]
							print("Changing: {0} TO {1}".format(block_id, replacements[block_id][block_data]["toID"]))
				except NameError:
					pass
				# If changes were made, update level variable for writing back to file
				if chunk_modified:					
					add = [0] * 4096
					for i,v in enumerate(blocks):
						if blocks[i] > 255:
							add[i] = blocks[i]//256
							blocks[i] -= add[i] * 256
					level["Sections"][ySec]["Add"] = array_byte_to_4bit(bytearray(add))
					level["Sections"][ySec]["Blocks"] = bytearray(blocks)
					level["Sections"][ySec]["Data"] = array_byte_to_4bit(data)
		try:
			replacements
			if chunk_modified:
				# Make add array if necessary
				# Get NBT form of arrays
				chunk["Level"] = pack_nbt(level)
				# Write out updated chunk
				print("Writing new data")
				region.write_chunk(abs(level["xPos"]), abs(level["zPos"]), chunk)
		except NameError as e:
			print("Error finding: {0}".format(e))
			pass
	return region_data

def main(world_folder, replacement_file_name):
	global replacements
	world = nbt.world.WorldFolder(world_folder)
	if not isinstance(world, nbt.world.AnvilWorldFolder):
		print("%s is not an Anvil world" % (world_folder))
		return 65 # EX_DATAERR
	if replacement_file_name != None:
		with open(replacement_file_name, 'r') as replacement_file:
			replacements = json.load(replacement_file)
	# get list of region files, going to pass this into function to process region
	region_files = world.get_regionfiles();
	
	# Parallel
	#p = Pool(processes=8)
	#region_data = p.map(process_region, region_files)
	# Not Parallel
	region_data = map(process_region, region_files)
	
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
	for block in world_data:
		#print("Block: {0}:{1} ID: {2} Data: {3}".format(block[0],block[1], block[2], json.dumps(block[3], default=to_json)))
		out_file.write("Block: {0}:{1} ID: {2} Data: {3}\n".format(block[0],block[1], block[2], json.dumps(block[3], default=to_json)))
	out_file.close()
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
