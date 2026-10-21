import csv,tempfile,unittest
from pathlib import Path
from prepare_wands import read_queries

class QueryReaderTests(unittest.TestCase):
    def test_csv_quoting_decodes_not_naive_splits(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'query.csv'
            path.write_text('query_id\tquery\tquery_class\n208\t"fawkes 36"" blue vanity"\tVanities\n285\t"48"" sliding single track , barn door for laundry"\tDoors\n391\t"writing desk 48"""\tDesks\n')
            self.assertEqual(read_queries(path),[{'query_id':'208','text':'fawkes 36" blue vanity'},{'query_id':'285','text':'48" sliding single track , barn door for laundry'},{'query_id':'391','text':'writing desk 48"'}])
    def test_embedded_tab_is_field_content(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'query.csv';path.write_text('query_id\tquery\tquery_class\n1\t"red\tchair"\tChair\n')
            self.assertEqual(read_queries(path),[{'query_id':'1','text':'red\tchair'}])
    def test_rejects_invalid_rows_ids_and_schemas(self):
        invalid=[
            'query_id\tquery\n1\tchair\textra\n',
            'query_id\tquery\n1\n',
            'query_id\tquery\n1\tchair\n1\tsofa\n',
            'query_id\tquery\n01\tchair\n',
            'query_id\tquery\n-1\tchair\n',
            'query_id\tquery\nabc\tchair\n',
            'query_id\tquery\n١\tchair\n',
            'query_id\tquery\n1\t  \n',
            'query_id\tquery\n',
            'query_id\tquery_id\tquery\n1\t2\tchair\n',
            'query_id\tquery\n1\t"unterminated\n',
            'query_id\tquery\n1\t"chair"oops\n',
        ]
        for contents in invalid:
            with self.subTest(contents=contents),tempfile.TemporaryDirectory() as d:
                path=Path(d)/'query.csv';path.write_text(contents)
                with self.assertRaises((ValueError,csv.Error)):read_queries(path)
if __name__=='__main__':unittest.main()
