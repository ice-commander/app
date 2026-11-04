package com.icecommander.gtkapp;

import org.gtk.android.ToplevelActivity;

/**
 * The activity the launcher opens. It exists only to carry singleTask: GTK starts any further
 * toplevel window with ToplevelActivity itself, which must stay standard for that to work.
 */
public class MainActivity extends ToplevelActivity {}
