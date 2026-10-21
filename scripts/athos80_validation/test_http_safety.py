import os
import unittest
import urllib.request
from unittest.mock import patch
from validate_http import validate_loopback,load_cases,loopback_opener,NoRedirect

class HttpSafetyTests(unittest.TestCase):
    def test_literal_only_explicit_origin(self):
        self.assertEqual(validate_loopback('http://127.0.0.1:18881/search'),('127.0.0.1',18881))
        for url in ['http://localhost:18881/search','http://example.com:80','http://192.0.2.1:80','https://127.0.0.1:18881/search','http://127.0.0.1/search','http://u:p@127.0.0.1:18881/search','http://127.0.0.1:18881/search?q=a']:
            with self.subTest(url=url),self.assertRaises(ValueError):validate_loopback(url)
    def test_empty_negative_cases_rejected(self):
        for package,n in [({'cases':[]},0),({'cases':[{}]},-1),({},0)]:
            with self.assertRaises(ValueError):load_cases(package,n)
        self.assertEqual(load_cases({'cases':[1,2]},1),[1])
    def test_environment_proxy_disabled(self):
        with patch.dict(os.environ,{'http_proxy':'http://example.com:8888','HTTP_PROXY':'http://example.com:8888'}):
            opener=loopback_opener()
            # ProxyHandler({}) adds no proxy_open handlers at all.
            self.assertFalse(any(isinstance(h,urllib.request.ProxyHandler) and h.proxies for h in opener.handlers))
    def test_redirects_declined(self):
        self.assertIsNone(NoRedirect().redirect_request(None,None,302,'redirect',{},'http://example.com/'))
if __name__=='__main__':unittest.main()
