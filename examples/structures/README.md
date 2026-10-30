# Demo proteins

Real protein structures for showing YUI at a demo or an exhibition. Open one with File → "Open Structure...".

All five files are unmodified downloads from the [RCSB Protein Data Bank](https://www.rcsb.org/) (`https://files.rcsb.org/download/<ID>.pdb`). The PDB archive data is released under CC0 1.0 (public domain).

| File | Protein | What to show | Atoms loaded |
|---|---|---|---|
| `1ema.pdb` | Green fluorescent protein (GFP) | The β-barrel with the chromophore (residue `CRO`) inside it. Select the chromophore and show it as sticks. | 1866 |
| `2hhb.pdb` | Human deoxyhemoglobin | Four chains and four heme groups (`HEM`). Colour by chain. | 4779 |
| `1zni.pdb` | Insulin with zinc ions | Small and light; the zinc ions (`ZN`) are easy to pick out. | 915 |
| `1bl8.pdb` | Potassium channel KcsA | Helices around a central pore, with potassium ions (`K`) in it. | 2824 |
| `1ubq.pdb` | Ubiquitin | The smallest and lightest; good for trying the controls and for slow computers. | 660 |

## Suggested order

1. `1ubq.pdb`: rotate, zoom and select, to show the controls.
2. `1ema.pdb`: the cartoon and the chromophore.
3. `2hhb.pdb`: a larger assembly, colour by chain.
4. `1bl8.pdb`: the pore and the ions.
5. Open the Ramachandran Plot (View menu) for any of them, and click a point to select the residue in the 3D view.

"Atoms loaded" is the number YUI reads; it can be smaller than the number of lines in the file because alternate locations are not read.
