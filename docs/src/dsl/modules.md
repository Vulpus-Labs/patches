# Modules & parameters

## Declaring a module

```patches
module <instance-name> : <ModuleType>
```

The instance name is local to the patch (or template). Module types are defined
by the module registry in `patches-modules`.

## Inline parameters

Parameters are supplied as a brace-enclosed comma-separated list:

```patches
module osc : Osc { frequency: 220Hz, drift: 0.5 }
```

Parameter names and allowed values are specific to each module type — see the
[Module reference](../modules/oscillators.md).

## Arity parameters

Some modules accept a variable number of ports. The arity is declared in
parentheses and cannot be changed by hot-reload (a change in arity replaces
the module):

```patches
module mix : StereoMixer(channels: 5)
```

## Indexed parameters

When a module has indexed ports or indexed parameters, use bracket notation:

```patches
module mix : StereoMixer(channels: 3) {
    level[0]: 0.8, pan[0]:  0.0,
    level[1]: 0.5, pan[1]: -0.7,
    level[2]: 0.5, pan[2]:  0.7
}
```
