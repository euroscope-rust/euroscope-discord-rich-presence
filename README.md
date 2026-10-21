# EuroScope Discord Rich Presence plugin

This is a EuroScope plugin to provide Discord Rich Presence information. It
displays your EuroScope status as an activity in Discord.

## Installing

1. Download the plugin DLL (for your EuroScope version) from the
   [Releases](https://github.com/euroscope-rust/euroscope-discord-rich-presence/releases).
   We currently publish for a few EuroScope versions which were available at the
   time of the release. Pick the one most approriate for you, although all the
   3.2.* versions should be compatible with each other.
2. Save it in a sensible place, like your EuroScope plugin directory
   (`%APPDATA%\EuroScope\Plugins`).
3. Load the plugin in EuroScope via `Other Set` > `Plugins`.

## Settings

To allow for maximum customisability, this plugin provides a number of settings
to control its behaviour.

The default settings are available at [`default.toml`](./default.toml) and are
sensible enough to get you running quickly. In that file, you'll also be able to
find documentation about what each setting does and how to customise to your
needs.

To load the settings, we look for a file named like the `.dll` plugin, but with
a `.toml` extension instead. For example, if your plugin is at
`%APPDATA%\EuroScope\Plugins\Discord\euroscope_discord_rich_presence.dll`, we
look for a settings file named
`%APPDATA%\EuroScope\Plugins\Discord\euroscope_discord_rich_presence.toml`.

### About assets

By default, the plugin will run with a "generic" Discord app, no tied to any
vACC specific entity. This means that the image shown in the sidebar when in a
voice channel will be the VATSIM logo and cannot be changed, only the images
displayed when viewing the activity in "full" can be customised. See the
[instructions below](#using-a-custom-discord-application) to create a Discord
app to change that.

If you do not need to customise that logo, but still want to change the other
images, you can use any of the assets specified below. If you want extra assets,
feel free to open an issue on this repository to request to add or update that
list.

The following assets are currently available:

- `vatsim`: the VATSIM logo
- `lsas`: the vACC Switzerland logo

There is a limit of 300 assets that Discord enforces per app, so we may not be
able to host many assets for your organisation.

You can also use a custom URL to set assets, most notably for moving assets
(like GIFs).

## Available commands

Commands are typed in the EuroScope command line and are case insensitive.

#### `.drp help`

Lists the available commands in the EuroScope message box, under the plugin's
own handler.

`.help drp` prints the same list under EuroScope's `HELP` handler, and a plain
`.help` makes the plugin announce itself there alongside the other plugins.

#### `.drp status`

Shows what the plugin is currently doing: whether processing is running or
stopped, which settings file is in use, whether we are connected to Discord,
the connection state we last saw, when the last update was pushed, when the next
one is due, and the payload that was last sent.

#### `.drp stop`

Stops processing: no further updates are sent to Discord and the Discord
connection is dropped, which clears the activity. The plugin also stops
gathering controller information while stopped, so it costs nothing to leave it
that way.

#### `.drp start`

Resumes processing after a `.drp stop`. The activity is pushed again from the
current state, and the connection to Discord is re-established.

#### `.drp reload`

Reloads the settings from the settings file on the fly, without having to unload
and re-load the plugin.

If the new settings are invalid, an error will be shown in the EuroScope message
box, and old settings will remain in place.

## Using a custom Discord application

Follow these instructions to create a custom Discord app for use with this
plugin:

1. Create a developer team on the [Discord Developer Portal](https://discord.com/developers/teams).
2. Create a new application on the [Discord Developer Portal](https://discord.com/developers/applications) and assign it to your team.
3. Enable the `Public Client` toggle in the [OAuth2 tab](https://discord.com/developers/applications/select/oauth2).
4. Write down the `Application ID` found in the [information tab](https://discord.com/developers/applications/select/information).
5. Create a custom settings file for this plugin with the following data:

```toml
[discord]
client_id = "<The application ID you copied in step 4>"

[activity.assets]
# Defaults to `vatsim`, which won't exist in your new application
large_image = "my-asset"
```

From the [information
tab](https://discord.com/developers/applications/select/information), you can
change your application icon, which is the icon that will show in the sidebar
when in a voice channel.

In the [Rich Presence Art Assets
tab](https://discord.com/developers/applications/select/rich-presence/assets),
you can upload extra assets to be used with the plugin. You can also customise
the cover image shown when opening the application in full.

## Contributing

Just open an issue or a PR :D

## Releasing

1. Run the [Release -
   Create](https://github.com/euroscope-rust/euroscope-discord-rich-presence/actions/workflows/release-create.yml)
   workflow, with the version to release.
2. Publish the created release.

## Acknowledgements

The idle texts come from the
[plugin](https://github.com/AlexisBalzano/EuroscopeRPC) used by the French vACC
and written by Alexis Balzano.