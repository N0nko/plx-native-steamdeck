#!/usr/bin/env python3
"""Render the vector startup mark with system librsvg/Cairo (Linux/WSL)."""
import ctypes as C
from ctypes.util import find_library
from pathlib import Path


def main():
    root = Path(__file__).resolve().parent.parent
    cairo = C.CDLL(find_library('cairo'))
    rsvg = C.CDLL(find_library('rsvg-2'))
    obj = C.CDLL(find_library('gobject-2.0'))
    def bind(lib, name, result, *args):
        fn = getattr(lib, name)
        fn.restype, fn.argtypes = result, list(args)
        return fn
    surface_new = bind(cairo, 'cairo_image_surface_create', C.c_void_p, C.c_int, C.c_int, C.c_int)
    context_new = bind(cairo, 'cairo_create', C.c_void_p, C.c_void_p)
    load = bind(rsvg, 'rsvg_handle_new_from_file', C.c_void_p, C.c_char_p, C.c_void_p)
    render = bind(rsvg, 'rsvg_handle_render_cairo', C.c_int, C.c_void_p, C.c_void_p)
    save = bind(cairo, 'cairo_surface_write_to_png', C.c_int, C.c_void_p, C.c_char_p)
    free_context = bind(cairo, 'cairo_destroy', None, C.c_void_p)
    free_surface = bind(cairo, 'cairo_surface_destroy', None, C.c_void_p)
    unref = bind(obj, 'g_object_unref', None, C.c_void_p)
    handle = load(str(root / 'assets/splash.svg').encode(), None)
    if not handle:
        raise RuntimeError('Cannot load splash.svg')
    surface = surface_new(0, 1920, 1080)
    context = context_new(surface)
    try:
        if not render(handle, context):
            raise RuntimeError('Cannot render splash.svg')
        if save(surface, str(root / 'pkg/splash.png').encode()):
            raise RuntimeError('Cannot save splash.png')
    finally:
        free_context(context)
        free_surface(surface)
        unref(handle)


if __name__ == '__main__':
    main()
