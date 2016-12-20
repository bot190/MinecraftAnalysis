from nbt.nbt import _TAG_End
def array_4bit_to_byte(array):
    """Convert a 2048-byte array of 4096 4-bit values to an array of 4096 1-byte values.
    The result is of type bytearray().
    Note that the first byte of the created arrays contains the LEAST significant 
    bits of the first byte of the Data. NOT to the MOST significant bits, as you 
    might expected. This is because Minecraft stores data in that way.
    """
    def iterarray(array):
        for b in array:
            yield(b & 15) # Little end of the byte
            yield((b >> 4) & 15) # Big end of the byte
    return bytearray(iterarray(array))

def array_byte_to_4bit(array):
    """Convert an array of 4096 1-byte values to a 2048-byte array of 4096 4-bit values.
    The result is of type bytearray().
    Any values larger than 16 are taken modulo 16.
    Note that the first byte of the original array will be placed in the LEAST 
    significant bits of the first byte of the result. Thus NOT to the MOST 
    significant bits, as you might expected. This is because Minecraft stores 
    data in that way.
    """
    def iterarray(array):
        arrayiter = iter(array)
        for b1 in arrayiter:
            b2 = next(arrayiter, 0)
            yield(((b2 & 15) << 4) + (b1 & 15))
    return bytearray(iterarray(array))

def unpack_nbt(tag):
    from nbt.nbt import NBTFile, TAG_Long, TAG_Int, TAG_String, TAG_List, TAG_Compound
    """
    Unpack an NBT tag into a native Python data structure.
    """

    if isinstance(tag, TAG_List):
        return [unpack_nbt(i) for i in tag.tags]
    elif isinstance(tag, TAG_Compound):
        return dict((i.name, unpack_nbt(i)) for i in tag.tags)
    else:
        return tag.value
    
def pack_nbt(s):
    from nbt.nbt import NBTFile, TAG_Long, TAG_Int, TAG_String, TAG_List, TAG_Compound, TAG_Byte, TAG_Double
    """
    Pack a native Python data structure into an NBT tag. Only the following
    structures and types are supported:
     * int
     * float
     * str
     * unicode
     * dict
    Additionally, arbitrary iterables are supported.
    Packing is not lossless. In order to avoid data loss, TAG_Long and
    TAG_Double are preferred over the less precise numerical formats.
    Lists and tuples may become dicts on unpacking if they were not homogenous
    during packing, as a side-effect of NBT's format. Nothing can be done
    about this.
    Only strings are supported as keys for dicts and other mapping types. If
    your keys are not strings, they will be coerced. (Resistance is futile.)
    """

    if isinstance(s, int):
        return TAG_Long(s)
    elif isinstance(s, float):
        return TAG_Double(s)
    elif isinstance(s, (str, unicode)):
        return TAG_String(s)
    elif isinstance(s, dict):
        tag = TAG_Compound()
        for k, v in s.items():
            v = pack_nbt(v)
            v.name = str(k)
            tag.tags.append(v)
        return tag
    elif hasattr(s, "__iter__"):
        # We arrive at a slight quandry. NBT lists must be homogenous, unlike
        # Python lists. NBT compounds work, but require unique names for every
        # entry. On the plus side, this technique should work for arbitrary
        # iterables as well.
        tags = [pack_nbt(i) for i in s]
        if (len(tags) == 0):
            # I think this is wrong...
            tag = TAG_List(type=type(TAG_Byte()))
            return tag
        t = type(tags[0])
        # If we're homogenous...
        if all(t == type(i) for i in tags):
            tag = TAG_List(type=t)
            tag.tags = tags
        else:
            tag = TAG_Compound()
            for i, item in enumerate(tags):
                item.name = str(i)
            tag.tags = tags
        return tag
    else:
        raise ValueError("Couldn't serialise type %s!" % type(s))
    
def to_json(python_object):
    if isinstance(python_object, bytearray):
        return {'__class__': 'bytearray',
                '__value__': list(python_object)}
    if isinstance(python_object, bytes):
        return {'__class__': 'bytes',
                '__value__': list(python_object)}
    raise TypeError(repr(python_object) + ' is not JSON serializable')
