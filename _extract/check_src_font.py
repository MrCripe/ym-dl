from fontTools.ttLib import TTFont
import os
f = TTFont(os.path.join(os.environ["TEMP"], "msr.ttf"))
g = f["GSUB"].table
print("features:", sorted({fr.FeatureTag for fr in g.FeatureList.FeatureRecord}))
print("lookup types:", {l.LookupType for l in g.LookupList.Lookup})
