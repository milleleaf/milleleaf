"""finance.openpyxl-edited.xlsx: Hong Kong set to 550,000 by a library that doesn't calculate, so the
total's cached result is dropped. openpyxl 3.1.5.   python3 openpyxl_edit.py finance.xlsx OUT.xlsx"""
import sys
import openpyxl

wb = openpyxl.load_workbook(sys.argv[1])
assert wb.active["A3"].value == "Hong Kong"
wb.active["B3"] = 550000
wb.save(sys.argv[2])
